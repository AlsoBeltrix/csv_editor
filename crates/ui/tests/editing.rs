//! Drives the whole interface headlessly: keys and typed text go in, the document changes.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use csv_editor_table::{CellRef, Document, ParseOptions};
use csv_editor_ui::{DocView, EditorApp, Services};
use egui::{Event, Key, Modifiers, vec2};
use egui_kittest::Harness;

fn app(text: &str) -> EditorApp {
    let mut app = EditorApp::new(Services::default());
    app.docs = vec![DocView::new(Document::from_bytes(text.as_bytes(), &ParseOptions::default()).unwrap())];
    app.active = 0;
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
    h.set_size(vec2(900.0, 600.0));
    h.run();
    h
}

fn type_text(h: &mut Harness<'_, EditorApp>, text: &str) {
    h.input_mut().events.push(Event::Text(text.to_string()));
    h.step();
}

fn press(h: &mut Harness<'_, EditorApp>, key: Key) {
    h.key_press(key);
    h.step();
}

fn press_with(h: &mut Harness<'_, EditorApp>, modifiers: Modifiers, key: Key) {
    h.key_press_modifiers(modifiers, key);
    h.step();
}

fn cell(h: &Harness<'_, EditorApp>, row: usize, col: usize) -> String {
    h.state().docs[0].doc.table.get(row, col).to_string()
}

fn active(h: &Harness<'_, EditorApp>) -> CellRef {
    h.state().docs[0].sel.active
}

#[test]
fn typing_enter_and_undo_edit_the_table() {
    let mut h = harness(app("h1,h2\r\n1,2\r\n3,4\r\n"));
    assert_eq!(active(&h), CellRef::new(1, 0), "the first body cell is selected");
    type_text(&mut h, "99");
    assert!(h.state().grid.editor.is_some(), "typing opens the editor");
    press(&mut h, Key::Enter);
    h.run();
    assert_eq!(cell(&h, 1, 0), "99");
    assert_eq!(active(&h), CellRef::new(2, 0), "Enter commits and moves down");
    assert!(h.state().docs[0].doc.is_dirty());
    press_with(&mut h, Modifiers::COMMAND, Key::Z);
    h.run();
    assert_eq!(cell(&h, 1, 0), "1");
    assert!(!h.state().docs[0].doc.is_dirty());
    press_with(&mut h, Modifiers::COMMAND, Key::Y);
    h.run();
    assert_eq!(cell(&h, 1, 0), "99");
}

#[test]
fn escape_cancels_and_arrows_commit_in_replace_mode() {
    let mut h = harness(app("a,b\r\n1,2\r\n"));
    type_text(&mut h, "zzz");
    press(&mut h, Key::Escape);
    h.run();
    assert_eq!(cell(&h, 1, 0), "1");
    assert!(h.state().grid.editor.is_none());
    type_text(&mut h, "7");
    press(&mut h, Key::ArrowRight);
    h.run();
    assert_eq!(cell(&h, 1, 0), "7");
    assert_eq!(active(&h), CellRef::new(1, 1));
    // Tab commits and moves right; Delete clears.
    type_text(&mut h, "8");
    assert!(h.state().grid.editor.is_some(), "typing after an arrow commit opens the editor again");
    press(&mut h, Key::Tab);
    h.run();
    assert_eq!(cell(&h, 1, 1), "8");
    assert_eq!(active(&h), CellRef::new(1, 2));
    press(&mut h, Key::ArrowLeft);
    press(&mut h, Key::Delete);
    h.run();
    assert_eq!(cell(&h, 1, 1), "");
}

#[test]
fn typing_past_the_table_grows_it_and_alt_arrows_move_rows() {
    let mut h = harness(app("a\r\nb\r\nc\r\n"));
    // Header on: body starts at row 1. Jump to the blank row after the data and type.
    press(&mut h, Key::ArrowDown);
    press(&mut h, Key::ArrowDown);
    assert_eq!(active(&h), CellRef::new(3, 0));
    type_text(&mut h, "d");
    press(&mut h, Key::Enter);
    h.run();
    assert_eq!(h.state().docs[0].doc.table.row_count(), 4);
    assert_eq!(cell(&h, 3, 0), "d");
    // Move row 4 ("d") up above "c".
    press(&mut h, Key::ArrowUp);
    press_with(&mut h, Modifiers::ALT, Key::ArrowUp);
    h.run();
    assert_eq!((cell(&h, 2, 0), cell(&h, 3, 0)), ("d".to_string(), "c".to_string()));
    assert_eq!(active(&h), CellRef::new(2, 0), "the selection travels with the row");
    assert_eq!(String::from_utf8(h.state().docs[0].doc.to_bytes().0).unwrap(), "a\r\nb\r\nd\r\nc\r\n");
}

#[test]
fn copy_cut_and_paste_move_blocks_of_cells() {
    let mut h = harness(app("a,b,c\r\n1,2,3\r\n4,5,6\r\n"));
    press_with(&mut h, Modifiers::SHIFT, Key::ArrowRight);
    h.input_mut().events.push(Event::Copy);
    h.step();
    let clip = h.state().grid.clip.clone().expect("Ctrl+C fills the clipboard");
    assert_eq!(clip.text, "1\t2");
    assert!(clip.cut.is_none());
    // Paste one row down: the system clipboard text matches, so the cells are used as they are.
    press(&mut h, Key::ArrowDown);
    h.input_mut().events.push(Event::Paste("1\t2".to_string()));
    h.step();
    h.run();
    assert_eq!((cell(&h, 2, 0), cell(&h, 2, 1)), ("1".to_string(), "2".to_string()));
    assert_eq!(h.state().docs[0].sel.bounds(), csv_editor_table::Range::new(CellRef::new(2, 0), CellRef::new(2, 1)));
    // Foreign text pastes as a block too, growing the table.
    press(&mut h, Key::ArrowDown);
    h.input_mut().events.push(Event::Paste("x\ty\r\np\tq\r\n".to_string()));
    h.step();
    h.run();
    assert_eq!(h.state().docs[0].doc.table.row_count(), 5);
    assert_eq!((cell(&h, 3, 0), cell(&h, 4, 1)), ("x".to_string(), "q".to_string()));
    // Cut moves: the source is emptied when pasted.
    h.input_mut().events.push(Event::Cut);
    h.step();
    press(&mut h, Key::ArrowRight);
    press(&mut h, Key::ArrowRight);
    let text = h.state().grid.clip.clone().unwrap().text;
    h.input_mut().events.push(Event::Paste(text));
    h.step();
    h.run();
    assert_eq!((cell(&h, 3, 0), cell(&h, 3, 1), cell(&h, 3, 2), cell(&h, 3, 3)), ("".to_string(), "".to_string(), "x".to_string(), "y".to_string()));
    assert_eq!(h.state().docs[0].doc.history.undo_label(), Some("Move"));
}

#[test]
fn a_new_document_starts_empty_and_saves_only_what_was_typed() {
    let mut h = harness(EditorApp::new(Services::default()));
    assert!(!h.state().docs[0].doc.dialect.header);
    type_text(&mut h, "x");
    press(&mut h, Key::Tab);
    type_text(&mut h, "y");
    press(&mut h, Key::Enter);
    h.run();
    assert_eq!(String::from_utf8(h.state().docs[0].doc.to_bytes().0).unwrap(), "x,y\r\n");
}
