//! The CSV editor's egui interface: toolbar, tabs, grid and status bar. Thin over
//! `csv_editor_table`: this crate holds interface state and drawing, the table crate holds the data.

pub mod find;
pub mod grid;
pub mod ops;
pub mod theme;
pub mod view;

use std::path::{Path, PathBuf};
use std::rc::Rc;

use csv_editor_table::{Document, ParseOptions};
use egui::{Id, Key, Modifiers, ViewportCommand};
use theme::Tokens;
pub use view::{DocView, Filter, FilterKind};

/// Host services the platform shell provides (file pickers). Absent on hosts without them.
#[derive(Default)]
pub struct Services {
    pub pick_open: Option<Box<dyn Fn() -> Vec<PathBuf>>>,
    /// Save dialog, given the suggested file name.
    pub pick_save: Option<Box<dyn Fn(&str) -> Option<PathBuf>>>,
    /// The system clipboard's text, for the menu's Paste.
    pub paste_text: Option<Rc<dyn Fn() -> Option<String>>>,
}

/// A "save changes?" question and what happens once it is answered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Prompt {
    /// Close this tab.
    Close(usize),
    /// Quit the application once every document is saved or abandoned.
    Quit,
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
    prompt: Option<Prompt>,
    /// The header rename dialog: column and the text being edited.
    rename: Option<(usize, String)>,
    /// Set once the user has answered every "save changes?" question before quitting.
    quit_confirmed: bool,
    pub find: find::FindBar,
    /// The row-filter dialog being edited.
    filter_dialog: Option<Filter>,
    title: String,
}

impl EditorApp {
    pub fn new(services: Services) -> Self {
        let mut grid = grid::GridState::default();
        grid.paste_text = services.paste_text.clone();
        EditorApp {
            dark: false,
            system_theme: true,
            theme_applied: None,
            grid,
            docs: vec![DocView::new(Document::new())],
            active: 0,
            services,
            message: None,
            prompt: None,
            rename: None,
            quit_confirmed: false,
            find: find::FindBar::default(),
            filter_dialog: None,
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

    /// Closes a tab, asking first when it has unsaved changes.
    pub fn request_close(&mut self, index: usize) {
        grid::commit_edit_in(&mut self.grid, self.docs.get_mut(index));
        if self.docs.get(index).is_some_and(|d| d.doc.is_dirty()) {
            self.active = index;
            self.prompt = Some(Prompt::Close(index));
        } else {
            self.close_document(index);
        }
    }

    /// Closes a tab without asking. There is always at least one document.
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

    /// Saves the document to its file, or asks where. Returns whether it was saved.
    pub fn save(&mut self, index: usize) -> bool {
        grid::commit_edit_in(&mut self.grid, self.docs.get_mut(index));
        match self.docs.get(index).and_then(|d| d.doc.path.clone()) {
            Some(path) => self.write_to(index, &path),
            None => self.save_as(index),
        }
    }

    pub fn save_as(&mut self, index: usize) -> bool {
        grid::commit_edit_in(&mut self.grid, self.docs.get_mut(index));
        let Some(view) = self.docs.get(index) else { return false };
        let name = view.doc.display_name();
        let suggested = if Path::new(&name).extension().is_some() { name } else { format!("{name}.csv") };
        let Some(path) = self.services.pick_save.as_ref().and_then(|pick| pick(&suggested)) else { return false };
        // Saving as .tsv means tabs; anything else keeps the delimiter the file came with.
        if path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("tsv") || e.eq_ignore_ascii_case("tab"))
            && let Some(view) = self.docs.get_mut(index)
        {
            view.doc.dialect.delimiter = b'\t';
        }
        self.write_to(index, &path)
    }

    fn write_to(&mut self, index: usize, path: &Path) -> bool {
        let Some(view) = self.docs.get_mut(index) else { return false };
        match view.doc.save_to(path) {
            Ok(0) => true,
            Ok(lost) => {
                let encoding = view.doc.dialect.encoding.label();
                self.message = Some((
                    "Saved with substitutions".to_string(),
                    format!("{lost} character(s) cannot be represented in {encoding} and were written as '?'. Save the file as UTF-8 to keep them."),
                ));
                true
            }
            Err(e) => {
                self.message = Some(("Can't save file".to_string(), e.to_string()));
                false
            }
        }
    }

    fn wants_dark(&self, ctx: &egui::Context) -> bool {
        if self.system_theme { ctx.system_theme().map(|t| t == egui::Theme::Dark).unwrap_or(self.dark) } else { self.dark }
    }

    /// Per-frame state updates that are not drawing: theme, window title, global shortcuts,
    /// the quit guard.
    pub fn logic(&mut self, ctx: &egui::Context) {
        let dark = self.wants_dark(ctx);
        if self.theme_applied != Some(dark) {
            theme::apply(ctx, dark);
            self.theme_applied = Some(dark);
        }
        if let Some(col) = self.grid.rename_request.take()
            && let Some(view) = self.active_view()
        {
            self.rename = Some((col, view.header_name(col).to_string()));
        }
        if let Some(col) = self.grid.filter_request.take()
            && let Some(view) = self.active_view()
        {
            let existing = view.filters.iter().find(|f| f.col == col).cloned();
            self.filter_dialog = Some(existing.unwrap_or(Filter { col, kind: FilterKind::Contains, text: String::new() }));
        }
        let dialog_open = self.message.is_some() || self.prompt.is_some() || self.rename.is_some() || self.filter_dialog.is_some();
        if !dialog_open {
            let (find, replace) = ctx.input_mut(|i| (i.consume_key(Modifiers::COMMAND, Key::F), i.consume_key(Modifiers::COMMAND, Key::H)));
            if find || replace {
                grid::commit_edit_in(&mut self.grid, self.docs.get_mut(self.active));
                self.find.show(replace);
            }
            if self.find.open
                && self.grid.editor.is_none()
                && ctx.memory(|m| m.focused().is_none_or(|id| id != Id::unique("find-query")))
                && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape))
            {
                self.find.close();
            }
        }
        if !dialog_open && self.grid.editor.is_none() {
            self.shortcuts(ctx);
        }
        if ctx.input(|i| i.viewport().close_requested())
            && !self.quit_confirmed
            && self.prompt.is_none()
            && let Some(i) = self.docs.iter().position(|d| d.doc.is_dirty())
        {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            self.active = i;
            self.prompt = Some(Prompt::Quit);
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

    fn shortcuts(&mut self, ctx: &egui::Context) {
        let app_keys: [(Modifiers, Key, fn(&mut EditorApp)); 9] = [
            (Modifiers::COMMAND, Key::O, |a| a.open_dialog()),
            (Modifiers::COMMAND, Key::N, |a| a.new_document()),
            (Modifiers::COMMAND, Key::W, |a| a.request_close(a.active)),
            (Modifiers::COMMAND, Key::S, |a| {
                a.save(a.active);
            }),
            (Modifiers::COMMAND | Modifiers::SHIFT, Key::S, |a| {
                a.save_as(a.active);
            }),
            (Modifiers::COMMAND, Key::Equals, |a| a.grid.zoom_step(1)),
            (Modifiers::COMMAND, Key::Plus, |a| a.grid.zoom_step(1)),
            (Modifiers::COMMAND, Key::Minus, |a| a.grid.zoom_step(-1)),
            (Modifiers::COMMAND, Key::Num0, |a| a.grid.set_zoom(1.0)),
        ];
        for (mods, key, action) in app_keys {
            if ctx.input_mut(|i| i.consume_key(mods, key)) {
                action(self);
            }
        }
        let EditorApp { grid, docs, active, .. } = self;
        let Some(view) = docs.get_mut(*active) else { return };
        let doc_keys: [(Modifiers, Key, fn(&mut grid::GridState, &mut DocView) -> bool); 3] = [
            (Modifiers::COMMAND, Key::Z, ops::undo),
            (Modifiers::COMMAND, Key::Y, ops::redo),
            (Modifiers::COMMAND | Modifiers::SHIFT, Key::Z, ops::redo),
        ];
        for (mods, key, action) in doc_keys {
            if ctx.input_mut(|i| i.consume_key(mods, key)) {
                action(grid, view);
            }
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
        if self.find.open {
            let EditorApp { find, grid, docs, active, .. } = self;
            if let Some(view) = docs.get_mut(*active) {
                egui::Panel::top("find")
                    .frame(
                        egui::Frame::new()
                            .fill(t.toolbar)
                            .inner_margin(egui::Margin::symmetric(8, 4))
                            .stroke(egui::Stroke::new(1.0, t.toolbar_border)),
                    )
                    .show(ui, |ui| find.ui(ui, grid, view, &t));
            }
        }
        egui::Panel::bottom("status")
            .frame(egui::Frame::new().fill(t.status_bar).inner_margin(egui::Margin::symmetric(8, 3)))
            .show(ui, |ui| self.status_bar(ui, &t));
        egui::CentralPanel::default().frame(egui::Frame::new().fill(t.grid_bg)).show(ui, |ui| grid::show(self, ui, &t));
        self.message_box(ui.ctx());
        self.save_prompt(ui.ctx());
        self.rename_dialog(ui.ctx());
        self.filter_dialog(ui.ctx());
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
                if ui.add(egui::Button::new("Save").shortcut_text("Ctrl+S")).clicked() {
                    ui.close();
                    self.save(self.active);
                }
                if ui.add(egui::Button::new("Save As…").shortcut_text("Ctrl+Shift+S")).clicked() {
                    ui.close();
                    self.save_as(self.active);
                }
                ui.separator();
                if ui.add(egui::Button::new("Close").shortcut_text("Ctrl+W")).clicked() {
                    self.request_close(self.active);
                    ui.close();
                }
                if ui.button("Quit").clicked() {
                    ui.ctx().send_viewport_cmd(ViewportCommand::Close);
                }
            });
            let EditorApp { grid, docs, active, .. } = self;
            if let Some(view) = docs.get_mut(*active) {
                ui.menu_button("Edit", |ui| ops::edit_menu(grid, view, ui));
                ui.menu_button("Rows", |ui| ops::rows_menu(grid, view, ui));
                ui.menu_button("Columns", |ui| ops::cols_menu(grid, view, ui));
                ui.menu_button("Data", |ui| ops::data_menu(grid, view, ui));
            }
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
            self.request_close(i);
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
            if view.filtered() {
                ui.colored_label(t.accent, format!("{} of {data_rows} rows shown × {} columns", view.visible_count(), table.width()));
            } else {
                ui.colored_label(t.text_dim, format!("{data_rows} rows × {} columns", table.width()));
            }
            if !sel.is_single_cell() {
                ui.separator();
                let cells: usize = sel.ranges.iter().map(|r| r.row_count().saturating_mul(r.col_count())).sum();
                ui.colored_label(t.text_dim, format!("{} × {} selected ({cells} cells)", bounds.row_count(), bounds.col_count()));
            }
            if view.doc.is_dirty() {
                ui.separator();
                ui.colored_label(t.text_dim, "Unsaved changes");
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.colored_label(t.text_dim, format!("{}%", (zoom * 100.0).round()));
                ui.separator();
                ui.colored_label(t.text_dim, format!("{}{}", csv_editor_table::column_label(sel.active.col), sel.active.row + 1));
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
                if ui.button("OK").clicked() || ui.input(|i| i.key_pressed(Key::Enter) || i.key_pressed(Key::Escape)) {
                    self.message = None;
                }
            });
        });
        if response.should_close() {
            self.message = None;
        }
    }

    fn save_prompt(&mut self, ctx: &egui::Context) {
        let Some(prompt) = self.prompt else { return };
        let index = match prompt {
            Prompt::Close(i) => i,
            Prompt::Quit => self.active,
        };
        let Some(name) = self.docs.get(index).map(|d| d.doc.display_name()) else {
            self.prompt = None;
            return;
        };
        #[derive(PartialEq)]
        enum Answer {
            Save,
            Discard,
            Cancel,
        }
        let mut answer = None;
        let response = egui::Modal::new(Id::unique("save-prompt")).show(ctx, |ui| {
            ui.set_width(380.0);
            ui.heading(format!("Save changes to {name}?"));
            ui.add_space(6.0);
            ui.label("Your changes will be lost if you don't save them.");
            ui.add_space(12.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Cancel").clicked() {
                    answer = Some(Answer::Cancel);
                }
                if ui.button("Don't save").clicked() {
                    answer = Some(Answer::Discard);
                }
                if ui.button("Save").clicked() {
                    answer = Some(Answer::Save);
                }
            });
            if ui.input(|i| i.key_pressed(Key::Escape)) {
                answer = Some(Answer::Cancel);
            }
        });
        if response.should_close() {
            answer = Some(Answer::Cancel);
        }
        let Some(answer) = answer else { return };
        let proceed = match answer {
            Answer::Cancel => {
                self.prompt = None;
                return;
            }
            Answer::Discard => true,
            Answer::Save => self.save(index),
        };
        if !proceed {
            // The save dialog was cancelled or failed: stay put, the prompt is over.
            self.prompt = None;
            return;
        }
        match prompt {
            Prompt::Close(i) => {
                self.close_document(i);
                self.prompt = None;
            }
            Prompt::Quit => {
                if answer == Answer::Discard
                    && let Some(view) = self.docs.get_mut(index)
                {
                    view.doc.history.mark_saved();
                }
                match self.docs.iter().position(|d| d.doc.is_dirty()) {
                    Some(next) => self.active = next,
                    None => {
                        self.prompt = None;
                        self.quit_confirmed = true;
                        ctx.send_viewport_cmd(ViewportCommand::Close);
                    }
                }
            }
        }
    }

    fn rename_dialog(&mut self, ctx: &egui::Context) {
        let Some((col, mut text)) = self.rename.take() else { return };
        let mut done = None;
        let response = egui::Modal::new(Id::unique("rename-header")).show(ctx, |ui| {
            ui.set_width(320.0);
            ui.heading(format!("Rename column {}", csv_editor_table::column_label(col)));
            ui.add_space(6.0);
            let edit = ui.add(egui::TextEdit::singleline(&mut text).desired_width(f32::INFINITY));
            if !ui.memory(|m| m.has_focus(edit.id)) && done.is_none() {
                edit.request_focus();
            }
            if edit.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                done = Some(true);
            }
            if ui.input(|i| i.key_pressed(Key::Escape)) {
                done = Some(false);
            }
            ui.add_space(10.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Cancel").clicked() {
                    done = Some(false);
                }
                if ui.button("Rename").clicked() {
                    done = Some(true);
                }
            });
        });
        if response.should_close() {
            done = Some(false);
        }
        match done {
            Some(true) => {
                if let Some(view) = self.active_view_mut() {
                    view.doc.set_cell(0, col, text.as_str());
                }
            }
            Some(false) => {}
            None => self.rename = Some((col, text)),
        }
    }

    fn filter_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut draft) = self.filter_dialog.take() else { return };
        let Some(view) = self.docs.get_mut(self.active) else { return };
        #[derive(PartialEq)]
        enum Done {
            Apply,
            Clear,
            Cancel,
        }
        let mut done = None;
        let width = view.doc.table.width().max(draft.col + 1);
        let response = egui::Modal::new(Id::unique("filter-dialog")).show(ctx, |ui| {
            ui.set_width(360.0);
            ui.heading("Filter rows");
            ui.add_space(6.0);
            egui::Grid::new("filter-grid").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                ui.label("Column");
                egui::ComboBox::from_id_salt("filter-col").selected_text(view.column_title(draft.col)).show_ui(ui, |ui| {
                    for c in 0..width.min(10_000) {
                        ui.selectable_value(&mut draft.col, c, view.column_title(c));
                    }
                });
                ui.end_row();
                ui.label("Show rows where it");
                egui::ComboBox::from_id_salt("filter-kind").selected_text(draft.kind.label()).show_ui(ui, |ui| {
                    for k in FilterKind::ALL {
                        ui.selectable_value(&mut draft.kind, k, k.label());
                    }
                });
                ui.end_row();
                if draft.kind.takes_text() {
                    ui.label("Text");
                    let edit = ui.add(egui::TextEdit::singleline(&mut draft.text).desired_width(f32::INFINITY));
                    if edit.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                        done = Some(Done::Apply);
                    }
                    ui.end_row();
                }
            });
            if ui.input(|i| i.key_pressed(Key::Escape)) {
                done = Some(Done::Cancel);
            }
            ui.add_space(10.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Cancel").clicked() {
                    done = Some(Done::Cancel);
                }
                if ui.add_enabled(view.filters.iter().any(|f| f.col == draft.col), egui::Button::new("Clear")).clicked() {
                    done = Some(Done::Clear);
                }
                if ui.button("Apply").clicked() {
                    done = Some(Done::Apply);
                }
            });
        });
        if response.should_close() {
            done = Some(Done::Cancel);
        }
        match done {
            Some(Done::Apply) => {
                view.set_filter(draft);
                self.grid.scroll_to_active = true;
            }
            Some(Done::Clear) => {
                view.clear_filter(draft.col);
                self.grid.scroll_to_active = true;
            }
            Some(Done::Cancel) => {}
            None => self.filter_dialog = Some(draft),
        }
    }
}
