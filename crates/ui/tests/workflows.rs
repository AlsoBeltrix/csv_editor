//! Regression coverage for document transitions and native-window decisions.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use csv_editor_table::{Document, ParseOptions};
use csv_editor_ui::{DocView, EditorApp, Services};
use egui::{Event, vec2};
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
