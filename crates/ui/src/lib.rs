//! The CSV editor's egui interface: toolbar, tabs, grid and status bar. Thin over
//! `csv_editor_table`: this crate holds interface state and drawing, the table crate holds the data.

pub mod find;
pub mod fonts;
pub mod grid;
pub mod ops;
pub mod theme;
pub mod view;

use std::path::{Path, PathBuf};
use std::rc::Rc;

use csv_editor_table::{Dialect, Document, Encoding, LineEnding, ParseOptions, Quoting};
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
    /// Set once the user has answered every "save changes?" question before quitting.
    quit_confirmed: bool,
    pub find: find::FindBar,
    /// The row-filter dialog being edited.
    filter_dialog: Option<Filter>,
    /// The dialect (delimiter and encoding) dialog being edited, with the "other" delimiter text.
    dialect_dialog: Option<(Dialect, String)>,
    /// The go-to-row dialog's text.
    goto: Option<String>,
    /// A short result shown in the status bar until this time (seconds).
    note: Option<(String, f64)>,
    /// Recently opened or saved files, most recent first. The shell persists it.
    pub recent: Vec<PathBuf>,
    shortcuts_open: bool,
    about_open: bool,
    fonts_installed: bool,
    title: String,
}

/// How many recent files the File menu remembers.
pub const RECENT_LIMIT: usize = 12;

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
            quit_confirmed: false,
            find: find::FindBar::default(),
            filter_dialog: None,
            dialect_dialog: None,
            goto: None,
            note: None,
            recent: Vec::new(),
            shortcuts_open: false,
            about_open: false,
            fonts_installed: false,
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
        let started = std::time::Instant::now();
        match Document::open(path, &ParseOptions::default()) {
            Ok(doc) => {
                log::info!("opened {} ({} rows × {} columns) in {:.2?}", path.display(), doc.table.row_count(), doc.table.width(), started.elapsed());
                if self.docs.len() == 1 && self.docs.first().is_some_and(DocView::is_blank) {
                    self.docs.clear();
                }
                self.docs.push(DocView::new(doc));
                self.active = self.docs.len() - 1;
                self.remember(path);
            }
            Err(e) => self.message = Some(("Can't open file".to_string(), e.to_string())),
        }
    }

    /// Puts the file at the top of the recent list.
    fn remember(&mut self, path: &Path) {
        self.recent.retain(|p| p != path);
        self.recent.insert(0, path.to_path_buf());
        self.recent.truncate(RECENT_LIMIT);
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
        let encoding = view.doc.dialect.encoding.label();
        let result = view.doc.save_to(path);
        if result.is_ok() {
            self.remember(path);
        }
        match result {
            Ok(0) => true,
            Ok(lost) => {
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
        if !self.fonts_installed {
            fonts::install(ctx);
            self.fonts_installed = true;
        }
        let dark = self.wants_dark(ctx);
        if self.theme_applied != Some(dark) {
            theme::apply(ctx, dark);
            self.theme_applied = Some(dark);
        }
        if let Some(col) = self.grid.filter_request.take()
            && let Some(view) = self.active_view()
        {
            let existing = view.filters.iter().find(|f| f.col == col).cloned();
            self.filter_dialog = Some(existing.unwrap_or(Filter { col, kind: FilterKind::Contains, text: String::new() }));
        }
        if std::mem::take(&mut self.grid.dialect_request)
            && let Some(view) = self.active_view()
        {
            let d = view.doc.dialect;
            let other = if Dialect::DELIMITERS.iter().any(|(b, _)| *b == d.delimiter) { String::new() } else { char::from(d.delimiter).to_string() };
            self.dialect_dialog = Some((d, other));
        }
        if std::mem::take(&mut self.grid.goto_request) {
            self.goto = Some(String::new());
        }
        if let Some(text) = self.grid.note.take() {
            self.note = Some((text, ctx.input(|i| i.time) + 5.0));
        }
        if self.note.as_ref().is_some_and(|(_, until)| ctx.input(|i| i.time) > *until) {
            self.note = None;
        }
        let dialog_open = self.message.is_some()
            || self.prompt.is_some()
            || self.filter_dialog.is_some()
            || self.dialect_dialog.is_some()
            || self.goto.is_some()
            || self.shortcuts_open
            || self.about_open;
        if !dialog_open && ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::G)) {
            self.goto = Some(String::new());
        }
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
        self.filter_dialog(ui.ctx());
        self.dialect_dialog(ui.ctx());
        self.goto_dialog(ui.ctx());
        self.help_dialogs(ui.ctx());
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
                ui.add_enabled_ui(!self.recent.is_empty(), |ui| {
                    ui.menu_button("Open recent", |ui| {
                        let mut open = None;
                        for p in &self.recent {
                            let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| p.display().to_string());
                            if ui.button(name).on_hover_text(p.display().to_string()).clicked() {
                                open = Some(p.clone());
                            }
                        }
                        ui.separator();
                        if ui.button("Clear list").clicked() {
                            self.recent.clear();
                            ui.close();
                        }
                        if let Some(p) = open {
                            ui.close();
                            self.open_path(&p);
                        }
                    });
                });
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
                if ui.button("Delimiter and encoding…").clicked() {
                    self.grid.dialect_request = true;
                    ui.close();
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
                    if ui
                        .checkbox(&mut header, "First row is header")
                        .on_hover_text("Sort, filters and cleanup leave row 1 alone; menus name columns by it")
                        .changed()
                    {
                        view.set_header(header);
                        ui.close();
                    }
                    if ui.checkbox(&mut view.freeze_top, "Freeze top row").changed() {
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
            ui.menu_button("Help", |ui| {
                if ui.button("Keyboard shortcuts").clicked() {
                    self.shortcuts_open = true;
                    ui.close();
                }
                if ui.button("About CSV Editor").clicked() {
                    self.about_open = true;
                    ui.close();
                }
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let Some(view) = self.active_view() {
                    let d = &view.doc.dialect;
                    let badge = format!("{} · {} · {}", d.delimiter_label(), d.encoding.label(), d.line_ending.label());
                    if ui
                        .add(egui::Button::new(egui::RichText::new(badge).color(t.text_dim)).frame(false))
                        .on_hover_text("Delimiter and encoding…")
                        .clicked()
                    {
                        self.grid.dialect_request = true;
                    }
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
        let data_rows = table.row_count().saturating_sub(view.data_start());
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
            if let Some((text, _)) = &self.note {
                ui.separator();
                ui.colored_label(t.accent, text);
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

    fn dialect_dialog(&mut self, ctx: &egui::Context) {
        let Some((mut draft, mut other)) = self.dialect_dialog.take() else { return };
        let Some(view) = self.docs.get_mut(self.active) else { return };
        let mut done = None;
        let response = egui::Modal::new(Id::unique("dialect-dialog")).show(ctx, |ui| {
            ui.set_width(400.0);
            ui.heading("Delimiter and encoding");
            ui.add_space(4.0);
            ui.label(egui::RichText::new("How the file is written when saved. Detected from the file when it was opened.").weak());
            ui.add_space(8.0);
            egui::Grid::new("dialect-grid").num_columns(2).spacing([10.0, 8.0]).show(ui, |ui| {
                ui.label("Delimiter");
                ui.horizontal(|ui| {
                    let known = Dialect::DELIMITERS.iter().any(|(b, _)| *b == draft.delimiter);
                    egui::ComboBox::from_id_salt("dialect-delim")
                        .selected_text(if known { draft.delimiter_label() } else { "Other".to_string() })
                        .show_ui(ui, |ui| {
                            for (b, label) in Dialect::DELIMITERS {
                                if ui.selectable_label(draft.delimiter == b, label).clicked() {
                                    draft.delimiter = b;
                                    other.clear();
                                }
                            }
                            if ui.selectable_label(!known, "Other…").clicked() && known {
                                draft.delimiter = b':';
                                other = ":".to_string();
                            }
                        });
                    if !known
                        && ui.add(egui::TextEdit::singleline(&mut other).desired_width(24.0).char_limit(1)).changed()
                        && let Some(c) = other.chars().next()
                        && c.is_ascii()
                        && !c.is_ascii_control()
                    {
                        draft.delimiter = c as u8;
                    }
                });
                ui.end_row();
                ui.label("Quote character");
                egui::ComboBox::from_id_salt("dialect-quote").selected_text(char::from(draft.quote).to_string()).show_ui(ui, |ui| {
                    ui.selectable_value(&mut draft.quote, b'"', "\" (double quote)");
                    ui.selectable_value(&mut draft.quote, b'\'', "' (single quote)");
                });
                ui.end_row();
                ui.label("Line endings");
                egui::ComboBox::from_id_salt("dialect-eol").selected_text(draft.line_ending.label()).show_ui(ui, |ui| {
                    for e in LineEnding::ALL {
                        ui.selectable_value(&mut draft.line_ending, e, e.label());
                    }
                });
                ui.end_row();
                ui.label("Encoding");
                egui::ComboBox::from_id_salt("dialect-enc").selected_text(draft.encoding.label()).show_ui(ui, |ui| {
                    for e in Encoding::ALL {
                        ui.selectable_value(&mut draft.encoding, e, e.label());
                    }
                });
                ui.end_row();
                ui.label("Quote fields");
                egui::ComboBox::from_id_salt("dialect-quoting").selected_text(draft.quoting.label()).show_ui(ui, |ui| {
                    ui.selectable_value(&mut draft.quoting, Quoting::Minimal, Quoting::Minimal.label());
                    ui.selectable_value(&mut draft.quoting, Quoting::All, Quoting::All.label());
                });
                ui.end_row();
            });
            ui.add_space(6.0);
            ui.checkbox(&mut draft.trailing_newline, "End the file with a line break");
            ui.checkbox(&mut draft.pad_rows, "Write every row with the same number of fields");
            ui.checkbox(&mut draft.header, "First row is the header");
            let invalid = draft.delimiter == draft.quote;
            if invalid {
                ui.colored_label(egui::Color32::from_rgb(0xC4, 0x2B, 0x1C), "The delimiter and the quote character must differ.");
            }
            if ui.input(|i| i.key_pressed(Key::Escape)) {
                done = Some(false);
            }
            ui.add_space(10.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Cancel").clicked() {
                    done = Some(false);
                }
                if ui.add_enabled(!invalid, egui::Button::new("Apply")).clicked() {
                    done = Some(true);
                }
            });
        });
        if response.should_close() {
            done = Some(false);
        }
        match done {
            Some(true) => {
                if draft.header != view.doc.dialect.header {
                    view.set_header(draft.header);
                }
                view.doc.dialect = draft;
            }
            Some(false) => {}
            None => self.dialect_dialog = Some((draft, other)),
        }
    }

    fn goto_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut text) = self.goto.take() else { return };
        let Some(view) = self.docs.get_mut(self.active) else { return };
        let mut done = None;
        let response = egui::Modal::new(Id::unique("goto-dialog")).show(ctx, |ui| {
            ui.set_width(260.0);
            ui.heading("Go to row");
            ui.add_space(6.0);
            let edit = ui.add(egui::TextEdit::singleline(&mut text).hint_text("row number").desired_width(f32::INFINITY));
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
                if ui.button("Go").clicked() {
                    done = Some(true);
                }
            });
        });
        if response.should_close() {
            done = Some(false);
        }
        match done {
            Some(true) => {
                if let Ok(n) = text.trim().parse::<usize>()
                    && n >= 1
                {
                    let row = view.snap_visible((n - 1).clamp(view.body_start(), view.max_cell().row));
                    view.sel = csv_editor_table::Selection::at(csv_editor_table::CellRef::new(row, view.sel.active.col));
                    self.grid.scroll_to_active = true;
                } else {
                    self.goto = Some(text);
                }
            }
            Some(false) => {}
            None => self.goto = Some(text),
        }
    }

    fn help_dialogs(&mut self, ctx: &egui::Context) {
        if self.shortcuts_open {
            let response = egui::Modal::new(Id::unique("shortcuts")).show(ctx, |ui| {
                ui.set_width(460.0);
                ui.heading("Keyboard shortcuts");
                ui.add_space(6.0);
                egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
                    egui::Grid::new("shortcuts-grid").num_columns(2).spacing([16.0, 4.0]).striped(true).show(ui, |ui| {
                        for (keys, what) in SHORTCUTS {
                            if keys.is_empty() {
                                ui.label(egui::RichText::new(*what).strong());
                                ui.label("");
                            } else {
                                ui.label(egui::RichText::new(*keys).monospace());
                                ui.label(*what);
                            }
                            ui.end_row();
                        }
                    });
                });
                ui.add_space(10.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Close").clicked() || ui.input(|i| i.key_pressed(Key::Escape) || i.key_pressed(Key::Enter)) {
                        self.shortcuts_open = false;
                    }
                });
            });
            if response.should_close() {
                self.shortcuts_open = false;
            }
        }
        if self.about_open {
            let response = egui::Modal::new(Id::unique("about")).show(ctx, |ui| {
                ui.set_width(380.0);
                ui.heading(format!("CSV Editor {}", env!("CARGO_PKG_VERSION")));
                ui.add_space(6.0);
                ui.label("A streamlined, spreadsheet-style editor for delimited text files. Every cell is text, and untouched files round-trip unchanged.");
                ui.add_space(6.0);
                ui.label(egui::RichText::new("Built with egui and eframe. Includes code adapted from GridCraft (MIT), listed in ATTRIBUTION.md and THIRD_PARTY_NOTICES.md.").weak());
                ui.add_space(10.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Close").clicked() || ui.input(|i| i.key_pressed(Key::Escape) || i.key_pressed(Key::Enter)) {
                        self.about_open = false;
                    }
                });
            });
            if response.should_close() {
                self.about_open = false;
            }
        }
    }
}

/// The shortcut list shown in Help. Empty keys start a section.
const SHORTCUTS: &[(&str, &str)] = &[
    ("", "Files"),
    ("Ctrl+N / Ctrl+O", "New document / Open files"),
    ("Ctrl+S / Ctrl+Shift+S", "Save / Save as"),
    ("Ctrl+W", "Close the document"),
    ("", "Moving around"),
    ("Arrows, Page Up/Down", "Move the active cell (Shift extends the selection)"),
    ("Ctrl+Arrows", "Jump to the edge of the data"),
    ("Home / End", "First column / last filled column of the row"),
    ("Ctrl+Home / Ctrl+End", "First cell / last cell of the data"),
    ("Tab, Enter", "Move right / down (Shift reverses; inside a selection they cycle)"),
    ("Ctrl+A", "Select the data block, then everything"),
    ("Ctrl+G", "Go to a row"),
    ("Ctrl+F / Ctrl+H", "Find / Find and replace"),
    ("Esc", "Cancel editing, close the find bar, collapse the selection"),
    ("", "Editing"),
    ("Type", "Replace the cell's text (arrows then commit and move)"),
    ("F2, double-click", "Edit the cell's text in place"),
    ("Alt+Enter", "Line break inside a cell"),
    ("Delete, Backspace", "Clear the selected cells"),
    ("Ctrl+D", "Fill the selection down from its first row"),
    ("Ctrl+Z / Ctrl+Y", "Undo / Redo"),
    ("Ctrl+X / Ctrl+C / Ctrl+V", "Cut / Copy / Paste (tab-separated text)"),
    ("Alt+Up / Alt+Down", "Move the selected rows"),
    ("Alt+Left / Alt+Right", "Move the selected columns"),
    ("", "View"),
    ("Ctrl+Wheel, Ctrl++ / Ctrl+- / Ctrl+0", "Zoom in / out / reset"),
    ("Double-click a column or row edge", "Fit the selected columns or rows to their contents"),
    ("Drag a column or row edge", "Resize"),
    ("Right-click", "Context menu for cells, rows (gutter) or columns (header)"),
];
