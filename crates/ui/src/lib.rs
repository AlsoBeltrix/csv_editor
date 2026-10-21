//! The CSV editor's egui interface: toolbar, grid and status bar. Thin over `csv_editor_table`.

pub mod grid;
pub mod theme;

use theme::Tokens;

/// The whole interface. The platform shell (`apps/csv-editor`) owns the window and calls
/// [`EditorApp::logic`] then [`EditorApp::ui`] every frame.
pub struct EditorApp {
    /// Manual theme preference, used when `system_theme` is off.
    pub dark: bool,
    pub system_theme: bool,
    theme_applied: Option<bool>,
    pub grid: grid::GridState,
}

impl Default for EditorApp {
    fn default() -> Self {
        Self::new()
    }
}

impl EditorApp {
    pub fn new() -> Self {
        EditorApp { dark: false, system_theme: true, theme_applied: None, grid: grid::GridState::default() }
    }

    /// Whether the interface should be dark this frame.
    fn wants_dark(&self, ctx: &egui::Context) -> bool {
        if self.system_theme { ctx.system_theme().map(|t| t == egui::Theme::Dark).unwrap_or(self.dark) } else { self.dark }
    }

    /// Per-frame state updates that are not drawing.
    pub fn logic(&mut self, ctx: &egui::Context) {
        let dark = self.wants_dark(ctx);
        if self.theme_applied != Some(dark) {
            theme::apply(ctx, dark);
            self.theme_applied = Some(dark);
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        let t = Tokens::get(ui.ctx());
        egui::Panel::top("toolbar")
            .frame(egui::Frame::new().fill(t.toolbar).inner_margin(egui::Margin::symmetric(8, 4)).stroke(egui::Stroke::new(1.0, t.toolbar_border)))
            .show(ui, |ui| self.toolbar(ui, &t));
        egui::Panel::bottom("status")
            .frame(egui::Frame::new().fill(t.status_bar).inner_margin(egui::Margin::symmetric(8, 3)))
            .show(ui, |ui| self.status_bar(ui, &t));
        egui::CentralPanel::default().frame(egui::Frame::new().fill(t.grid_bg)).show(ui, |ui| grid::show(self, ui, &t));
    }

    fn toolbar(&mut self, ui: &mut egui::Ui, _t: &Tokens) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui.button("Quit").clicked() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
            ui.menu_button("View", |ui| {
                if ui.checkbox(&mut self.system_theme, "Follow system theme").changed() {
                    ui.close();
                }
                ui.add_enabled_ui(!self.system_theme, |ui| {
                    if ui.checkbox(&mut self.dark, "Dark").changed() {
                        ui.close();
                    }
                });
            });
        });
    }

    fn status_bar(&mut self, ui: &mut egui::Ui, t: &Tokens) {
        ui.horizontal(|ui| {
            ui.colored_label(t.text_dim, "Ready");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.colored_label(t.text_dim, "No file");
            });
        });
    }
}
