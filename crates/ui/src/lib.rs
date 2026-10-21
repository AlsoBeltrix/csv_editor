//! The CSV editor's egui interface: toolbar, tabs, grid and status bar. Thin over
//! `csv_editor_table`: this crate holds interface state and drawing, the table crate holds the data.

pub mod grid;
pub mod theme;
pub mod view;

use std::path::{Path, PathBuf};

use csv_editor_table::{Document, ParseOptions};
use egui::{Id, Key, Modifiers, ViewportCommand};
use theme::Tokens;
pub use view::DocView;

/// Host services the platform shell provides (file pickers). Absent on hosts without them.
#[derive(Default)]
pub struct Services {
    pub pick_open: Option<Box<dyn Fn() -> Vec<PathBuf>>>,
    /// Save dialog, given the suggested file name.
    pub pick_save: Option<Box<dyn Fn(&str) -> Option<PathBuf>>>,
}

/// The whole interface. The platform shell (`apps/csv-editor`) owns the window and calls
/// [`EditorApp::logic`] then [`EditorApp::ui`] every frame.
pub struct EditorApp {
    /// Manual theme preference, used when `system_theme` is off.
    pub dark: bool,
    pub system_theme: bool,
    theme_applied: Option<bool>,
    pub grid: grid::GridState,
    pub docs: Vec<DocView>,
    pub active: usize,
    pub services: Services,
    /// A message box: title and text.
    pub message: Option<(String, String)>,
    title: String,
}

impl EditorApp {
    pub fn new(services: Services) -> Self {
        EditorApp {
            dark: false,
            system_theme: true,
            theme_applied: None,
            grid: grid::GridState::default(),
            docs: vec![DocView::new(Document::new())],
            active: 0,
            services,
            message: None,
            title: String::new(),
        }
    }

    pub fn active_view(&self) -> Option<&DocView> {
        self.docs.get(self.active)
    }

    pub fn active_view_mut(&mut self) -> Option<&mut DocView> {
        self.docs.get_mut(self.active)
    }

    /// Opens a file in a new tab (or switches to it if it is already open). Errors go to a
    /// message box.
    pub fn open_path(&mut self, path: &Path) {
        if let Some(i) = self.docs.iter().position(|d| d.doc.path.as_deref() == Some(path)) {
            self.active = i;
            return;
        }
        match Document::open(path, &ParseOptions::default()) {
            Ok(doc) => {
                if self.docs.len() == 1 && self.docs.first().is_some_and(DocView::is_blank) {
                    self.docs.clear();
                }
                self.docs.push(DocView::new(doc));
                self.active = self.docs.len() - 1;
            }
            Err(e) => self.message = Some(("Can't open file".to_string(), e.to_string())),
        }
    }

    pub fn new_document(&mut self) {
        self.docs.push(DocView::new(Document::new()));
        self.active = self.docs.len() - 1;
    }

    /// Closes a tab. There is always at least one document.
    pub fn close_document(&mut self, index: usize) {
        if index < self.docs.len() {
            self.docs.remove(index);
        }
        if self.docs.is_empty() {
            self.docs.push(DocView::new(Document::new()));
        }
        self.active = self.active.min(self.docs.len() - 1);
    }

    fn open_dialog(&mut self) {
        let paths = self.services.pick_open.as_ref().map(|pick| pick()).unwrap_or_default();
        for p in paths {
            self.open_path(&p);
        }
    }

    fn wants_dark(&self, ctx: &egui::Context) -> bool {
        if self.system_theme { ctx.system_theme().map(|t| t == egui::Theme::Dark).unwrap_or(self.dark) } else { self.dark }
    }

    /// Per-frame state updates that are not drawing: theme, window title, global shortcuts.
    pub fn logic(&mut self, ctx: &egui::Context) {
        let dark = self.wants_dark(ctx);
        if self.theme_applied != Some(dark) {
            theme::apply(ctx, dark);
            self.theme_applied = Some(dark);
        }
        let shortcuts: [(Key, fn(&mut EditorApp)); 7] = [
            (Key::O, |a| a.open_dialog()),
            (Key::N, |a| a.new_document()),
            (Key::W, |a| a.close_document(a.active)),
            (Key::Equals, |a| a.grid.zoom_step(1)),
            (Key::Plus, |a| a.grid.zoom_step(1)),
            (Key::Minus, |a| a.grid.zoom_step(-1)),
            (Key::Num0, |a| a.grid.set_zoom(1.0)),
        ];
        for (key, action) in shortcuts {
            if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, key)) {
                action(self);
            }
        }
        let title = match self.active_view() {
            Some(v) => format!("{}{} - CSV Editor", v.doc.display_name(), if v.doc.is_dirty() { "*" } else { "" }),
            None => "CSV Editor".to_string(),
        };
        if title != self.title {
            ctx.send_viewport_cmd(ViewportCommand::Title(title.clone()));
            self.title = title;
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        let t = Tokens::get(ui.ctx());
        egui::Panel::top("toolbar")
            .frame(egui::Frame::new().fill(t.toolbar).inner_margin(egui::Margin::symmetric(8, 4)).stroke(egui::Stroke::new(1.0, t.toolbar_border)))
            .show(ui, |ui| self.toolbar(ui, &t));
        if self.docs.len() > 1 {
            egui::Panel::top("tabs")
                .frame(egui::Frame::new().fill(t.tab_bar).inner_margin(egui::Margin::symmetric(6, 2)))
                .show(ui, |ui| self.tabs(ui, &t));
        }
        egui::Panel::bottom("status")
            .frame(egui::Frame::new().fill(t.status_bar).inner_margin(egui::Margin::symmetric(8, 3)))
            .show(ui, |ui| self.status_bar(ui, &t));
        egui::CentralPanel::default().frame(egui::Frame::new().fill(t.grid_bg)).show(ui, |ui| grid::show(self, ui, &t));
        self.message_box(ui.ctx());
    }

    fn toolbar(&mut self, ui: &mut egui::Ui, t: &Tokens) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui.add(egui::Button::new("New").shortcut_text("Ctrl+N")).clicked() {
                    self.new_document();
                    ui.close();
                }
                if ui.add(egui::Button::new("Open…").shortcut_text("Ctrl+O")).clicked() {
                    ui.close();
                    self.open_dialog();
                }
                ui.separator();
                if ui.add(egui::Button::new("Close").shortcut_text("Ctrl+W")).clicked() {
                    self.close_document(self.active);
                    ui.close();
                }
                if ui.button("Quit").clicked() {
                    ui.ctx().send_viewport_cmd(ViewportCommand::Close);
                }
            });
            ui.menu_button("View", |ui| {
                if ui.add(egui::Button::new("Zoom in").shortcut_text("Ctrl++")).clicked() {
                    self.grid.zoom_step(1);
                }
                if ui.add(egui::Button::new("Zoom out").shortcut_text("Ctrl+-")).clicked() {
                    self.grid.zoom_step(-1);
                }
                if ui.add(egui::Button::new("Reset zoom").shortcut_text("Ctrl+0")).clicked() {
                    self.grid.set_zoom(1.0);
                    ui.close();
                }
                ui.separator();
                if let Some(view) = self.docs.get_mut(self.active) {
                    let mut header = view.doc.dialect.header;
                    if ui.checkbox(&mut header, "First row is header").changed() {
                        view.set_header(header);
                        ui.close();
                    }
                }
                ui.separator();
                if ui.checkbox(&mut self.system_theme, "Follow system theme").changed() {
                    ui.close();
                }
                ui.add_enabled_ui(!self.system_theme, |ui| {
                    if ui.checkbox(&mut self.dark, "Dark").changed() {
                        ui.close();
                    }
                });
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let Some(view) = self.active_view() {
                    let d = &view.doc.dialect;
                    ui.colored_label(t.text_dim, format!("{} · {} · {}", d.delimiter_label(), d.encoding.label(), d.line_ending.label()));
                }
            });
        });
    }

    fn tabs(&mut self, ui: &mut egui::Ui, t: &Tokens) {
        let mut close = None;
        ui.horizontal(|ui| {
            for (i, view) in self.docs.iter().enumerate() {
                let label = format!("{}{}", view.doc.display_name(), if view.doc.is_dirty() { " •" } else { "" });
                let resp = ui.selectable_label(i == self.active, label);
                if resp.clicked() {
                    self.active = i;
                }
                if resp.middle_clicked() || ui.add(egui::Button::new("×").frame(false).small()).clicked() {
                    close = Some(i);
                }
                ui.add_space(2.0);
                ui.painter().vline(ui.cursor().left(), ui.cursor().y_range(), egui::Stroke::new(1.0, t.separator));
                ui.add_space(4.0);
            }
        });
        if let Some(i) = close {
            self.close_document(i);
        }
    }

    fn status_bar(&mut self, ui: &mut egui::Ui, t: &Tokens) {
        let zoom = self.grid.zoom;
        let Some(view) = self.active_view() else { return };
        let table = &view.doc.table;
        let data_rows = table.row_count().saturating_sub(view.body_start());
        let sel = &view.sel;
        let bounds = sel.bounds();
        ui.horizontal(|ui| {
            ui.colored_label(t.text_dim, format!("{data_rows} rows × {} columns", table.width()));
            if !sel.is_single_cell() {
                ui.separator();
                let cells: usize = sel.ranges.iter().map(|r| r.row_count().saturating_mul(r.col_count())).sum();
                ui.colored_label(t.text_dim, format!("{} × {} selected ({cells} cells)", bounds.row_count(), bounds.col_count()));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.colored_label(t.text_dim, format!("{}%", (zoom * 100.0).round()));
                ui.separator();
                ui.colored_label(t.text_dim, format!("{}{}", column_label_for(sel.active.col), sel.active.row + 1));
            });
        });
    }

    fn message_box(&mut self, ctx: &egui::Context) {
        let Some((title, body)) = self.message.clone() else { return };
        let response = egui::Modal::new(Id::unique("message-box")).show(ctx, |ui| {
            ui.set_width(380.0);
            ui.heading(title);
            ui.add_space(6.0);
            ui.label(body);
            ui.add_space(12.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("OK").clicked() {
                    self.message = None;
                }
            });
        });
        if response.should_close() {
            self.message = None;
        }
    }
}

fn column_label_for(col: usize) -> String {
    csv_editor_table::column_label(col)
}
