//! Regression coverage for document transitions and native-window decisions.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use csv_editor_table::{CellRef, Document, ParseOptions, Range, Selection};
use csv_editor_ui::{DocView, EditorApp, Services, ops};
use egui::{Event, Modifiers, PointerButton, ViewportCommand, ViewportEvent, ViewportId, vec2};
use egui_kittest::{Harness, kittest::Queryable};

fn view(text: &str) -> DocView {
    DocView::new(Document::from_bytes(text.as_bytes(), &ParseOptions { header: Some(false), ..Default::default() }).unwrap())
}

fn app(text: &str) -> EditorApp {
    let mut app = EditorApp::new(Services::default());
    app.docs = vec![view(text)];
    app
}

fn harness(app: EditorApp) -> Harness<'static, EditorApp> {
    let mut h = Harness::new_ui_state(
        |ui, app: &mut EditorApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.set_size(vec2(1000.0, 650.0));
    h.run();
    h
}

fn type_text(h: &mut Harness<'_, EditorApp>, text: &str) {
    h.input_mut().events.push(Event::Text(text.to_owned()));
    h.step();
    h.run();
    assert!(h.state().grid.editor.is_some());
}

#[test]
fn active_cell_editor_has_an_accessible_name() {
    let mut h = harness(app("original\r\n"));
    type_text(&mut h, "unsaved");
    h.get_by_label("Cell A1");
}

struct TempCsv(std::path::PathBuf);

impl TempCsv {
    fn new(text: &str) -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let sequence = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("csv-editor-workflow-{}-{stamp}-{sequence}.csv", std::process::id()));
        std::fs::write(&path, text).unwrap();
        Self(path)
    }
}

impl Drop for TempCsv {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[test]
fn document_transition_tab_click_commits_to_the_original_document() {
    let mut a = app("original\r\n");
    a.docs[0].doc.path = Some("first.csv".into());
    let mut second = view("second\r\n");
    second.doc.path = Some("second.csv".into());
    a.docs.push(second);
    let mut h = harness(a);
    type_text(&mut h, "unsaved");
    let pos = h.get_by_label("second.csv").rect().center();
    h.input_mut().events.push(Event::PointerMoved(pos));
    h.input_mut().events.push(Event::PointerButton { pos, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.step();
    h.input_mut().events.push(Event::PointerButton { pos, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.step();
    h.run();
    assert_eq!(h.state().active, 1);
    assert_eq!((h.state().docs[0].doc.table.get(0, 0), h.state().docs[1].doc.table.get(0, 0)), ("unsaved", "second"));
    assert!(!h.state().docs[1].doc.is_dirty());
    h.state_mut().docs[0].doc.undo();
    assert_eq!(h.state().docs[0].doc.table.get(0, 0), "original");
}

#[test]
fn document_transition_open_and_reopen_preserve_the_edit_owner() {
    let file = TempCsv::new("dropped\r\n");
    let mut h = harness(app("original\r\n"));
    type_text(&mut h, "unsaved");
    // The native drop handler calls this method before drawing the next frame.
    h.state_mut().open_path(&file.0);
    h.run();
    assert_eq!((h.state().docs[0].doc.table.get(0, 0), h.state().docs[1].doc.table.get(0, 0)), ("unsaved", "dropped"));
    h.state_mut().new_document();
    h.run();
    type_text(&mut h, "new file");
    h.state_mut().open_path(&file.0);
    h.run();
    assert_eq!(h.state().active, 1);
    assert_eq!(h.state().docs[2].doc.table.get(0, 0), "new file");
    assert_eq!(h.state().docs[1].doc.table.get(0, 0), "dropped");
}

#[test]
fn document_transition_new_and_first_open_keep_uncommitted_untitled_data() {
    let mut h = harness(EditorApp::new(Services::default()));
    type_text(&mut h, "first");
    h.state_mut().new_document();
    h.run();
    assert_eq!(h.state().docs[0].doc.table.get(0, 0), "first");
    assert!(h.state().docs[1].doc.table.is_empty());

    let file = TempCsv::new("opened\r\n");
    let mut h = harness(EditorApp::new(Services::default()));
    type_text(&mut h, "keep me");
    h.state_mut().open_path(&file.0);
    h.run();
    assert_eq!(h.state().docs.len(), 2, "the untitled edit must not be discarded as a blank document");
    assert_eq!(h.state().docs[0].doc.table.get(0, 0), "keep me");
    assert_eq!(h.state().docs[1].doc.table.get(0, 0), "opened");
}

#[test]
fn document_transition_closing_a_background_tab_keeps_the_active_edit() {
    let mut a = app("background\r\n");
    a.docs.push(view("active\r\n"));
    a.active = 1;
    let mut h = harness(a);
    type_text(&mut h, "keep me");
    h.state_mut().request_close(0);
    h.run();
    assert_eq!(h.state().docs.len(), 1);
    assert_eq!(h.state().active, 0);
    assert_eq!(h.state().docs[0].doc.table.get(0, 0), "keep me");
    assert!(h.state().docs[0].doc.is_dirty());
}

#[test]
fn document_transition_saving_a_background_tab_keeps_the_active_edit() {
    let file = TempCsv::new("background\r\n");
    let mut a = app("background\r\n");
    a.docs[0].doc.path = Some(file.0.clone());
    a.docs.push(view("active\r\n"));
    a.active = 1;
    let mut h = harness(a);
    type_text(&mut h, "keep me");
    assert!(h.state_mut().save(0));
    h.run();
    assert_eq!(std::fs::read_to_string(&file.0).unwrap(), "background\r\n");
    assert_eq!(h.state().docs[1].doc.table.get(0, 0), "keep me");
    assert!(h.state().docs[1].doc.is_dirty());
}

fn close_request(h: &mut Harness<'_, EditorApp>) {
    let info = h.input_mut().viewports.get_mut(&ViewportId::ROOT).unwrap();
    info.events.clear();
    info.events.push(ViewportEvent::Close);
    h.step();
    h.input_mut().viewports.get_mut(&ViewportId::ROOT).unwrap().events.clear();
}

fn command(h: &Harness<'_, EditorApp>, wanted: ViewportCommand) -> bool {
    h.output().viewport_output.get(&ViewportId::ROOT).unwrap().commands.contains(&wanted)
}

#[test]
fn close_guard_includes_the_active_cell_edit_and_allows_cancel() {
    let mut h = harness(app("original\r\n"));
    type_text(&mut h, "unsaved");
    assert!(!h.state().docs[0].doc.is_dirty(), "the pending editor has not committed yet");
    close_request(&mut h);
    assert!(command(&h, ViewportCommand::CancelClose));
    assert_eq!(h.state().docs[0].doc.table.get(0, 0), "unsaved");
    h.run();
    h.get_by_label("Cancel").click();
    h.run();
    assert!(h.state().docs[0].doc.is_dirty());
    assert!(!command(&h, ViewportCommand::Close));
}

#[test]
fn close_guard_handles_repeated_window_and_tab_close_prompts() {
    for tab_prompt in [false, true] {
        let mut a = app("original\r\n");
        a.docs[0].doc.set_cell(0, 0, "unsaved");
        let mut h = harness(a);
        if tab_prompt {
            h.state_mut().request_close(0);
        } else {
            close_request(&mut h);
            assert!(command(&h, ViewportCommand::CancelClose));
        }
        h.run();
        close_request(&mut h);
        assert!(command(&h, ViewportCommand::CancelClose), "an existing prompt must protect every close request");
        assert!(h.state().docs[0].doc.is_dirty());
    }
}

#[test]
fn close_guard_allows_an_unchanged_document_to_close() {
    let mut h = harness(app("original\r\n"));
    close_request(&mut h);
    assert!(!command(&h, ViewportCommand::CancelClose));
}

#[test]
fn cut_guard_other_documents_receive_an_undoable_copy() {
    let mut a = app("source,keep\r\n");
    a.docs.push(view("precious,target\r\n"));
    ops::cut(&mut a.grid, &mut a.docs[0], &egui::Context::default());
    a.docs[1].sel = Selection::at(CellRef::new(0, 1));
    let text = a.grid.clip.as_ref().unwrap().text.clone();
    assert!(ops::paste(&mut a.grid, &mut a.docs[1], Some(text)));
    assert_eq!((a.docs[1].doc.table.get(0, 0), a.docs[1].doc.table.get(0, 1)), ("precious", "source"));
    assert_eq!(a.docs[0].doc.table.get(0, 0), "source");
    assert!(!a.docs[0].doc.is_dirty());
    assert_eq!(a.docs[1].doc.undo().as_deref(), Some("Paste"));
    assert_eq!((a.docs[1].doc.table.get(0, 0), a.docs[1].doc.table.get(0, 1)), ("precious", "target"));
    assert!(a.grid.note.as_deref().unwrap().contains("copy"));
    assert!(a.grid.clip.as_ref().unwrap().cut.is_none());
}

#[test]
fn cut_guard_changed_or_closed_sources_cannot_erase_new_cells() {
    for close_source in [false, true] {
        let mut a = app("source,keep\r\n");
        ops::cut(&mut a.grid, &mut a.docs[0], &egui::Context::default());
        if close_source {
            a.close_document(0);
        } else {
            a.docs[0].doc.insert_rows(0, 1);
        }
        a.docs[0].doc.set_cell(0, 0, "new data");
        a.docs[0].sel = Selection::at(CellRef::new(2, 1));
        let text = a.grid.clip.as_ref().unwrap().text.clone();
        assert!(ops::paste(&mut a.grid, &mut a.docs[0], Some(text)));
        assert_eq!(a.docs[0].doc.table.get(0, 0), "new data");
        assert_eq!(a.docs[0].doc.table.get(2, 1), "source");
        assert_eq!(a.docs[0].doc.undo().as_deref(), Some("Paste"));
        assert_eq!(a.docs[0].doc.table.get(0, 0), "new data");
        if !close_source {
            assert_eq!(a.docs[0].doc.table.get(1, 0), "source");
        }
    }
}

#[test]
fn cut_guard_same_document_overlap_moves_once_and_undo_restores_it() {
    let mut a = app("1,2,3\r\n");
    a.docs[0].sel = Selection::range(Range::new(CellRef::new(0, 0), CellRef::new(0, 1)));
    ops::cut(&mut a.grid, &mut a.docs[0], &egui::Context::default());
    let text = a.grid.clip.as_ref().unwrap().text.clone();
    a.docs[0].sel = Selection::at(CellRef::new(0, 1));
    assert!(ops::paste(&mut a.grid, &mut a.docs[0], Some(text.clone())));
    assert_eq!(String::from_utf8(a.docs[0].doc.to_bytes().0).unwrap(), ",1,2\r\n");
    assert_eq!(a.docs[0].doc.undo().as_deref(), Some("Move"));
    assert_eq!(String::from_utf8(a.docs[0].doc.to_bytes().0).unwrap(), "1,2,3\r\n");
    a.docs[0].doc.redo();
    a.docs[0].sel = Selection::at(CellRef::new(1, 0));
    assert!(ops::paste(&mut a.grid, &mut a.docs[0], Some(text)));
    assert_eq!(String::from_utf8(a.docs[0].doc.to_bytes().0).unwrap(), ",1,2\r\n1,2,\r\n");
    assert_eq!(a.docs[0].doc.history.undo_label(), Some("Paste"));
}
