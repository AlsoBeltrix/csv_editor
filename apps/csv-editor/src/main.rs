//! CSV editor desktop app.
//!
//! Usage: `csv-editor [files…]`
#![cfg_attr(all(target_os = "windows", not(debug_assertions)), windows_subsystem = "windows")]

#[cfg(any(target_os = "windows", test))]
mod graphics;

use csv_editor_ui::EditorApp;

struct App(EditorApp);

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.0.logic(ctx);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.0.ui(ui);
    }
}

fn main() -> eframe::Result {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let files: Vec<String> = std::env::args().skip(1).collect();
    if !files.is_empty() {
        // Opening files named on the command line lands with the grid slice.
        log::info!("files named on the command line: {files:?}");
    }
    let app = EditorApp::new();
    let viewport = egui::ViewportBuilder::default()
        .with_title("CSV Editor")
        .with_inner_size([1280.0, 800.0])
        .with_min_inner_size([560.0, 360.0])
        .with_app_id("csv-editor")
        .with_drag_and_drop(true);
    let options = eframe::NativeOptions { viewport, ..Default::default() };
    // Before eframe creates the wgpu instance: Windows starts on DirectX 12, or OpenGL when
    // DirectX 12 has no GPU driver, and never touches Vulkan unless asked (see graphics.rs).
    #[cfg(target_os = "windows")]
    let backend_override = eframe::wgpu::Backends::from_env();
    #[cfg(target_os = "windows")]
    let (options, first_backends) = {
        let mut options = options;
        let backends = graphics::configure(&mut options, backend_override);
        (options, backends)
    };
    #[cfg(target_os = "windows")]
    let retry_options = options.clone();
    // The app waits here until a start gets as far as creating it; a start that fails before
    // (no graphics adapter, device or surface) leaves it here for the next attempt.
    let pending = std::cell::Cell::new(Some(app));
    let run = |options: eframe::NativeOptions| {
        eframe::run_native(
            "csv-editor",
            options,
            Box::new(|_cc| {
                let Some(app) = pending.take() else {
                    return Err("the app was already created".into());
                };
                Ok(Box::new(App(app)))
            }),
        )
    };
    let result = run(options);
    // A graphics error before the app existed: start once more with the other backend.
    #[cfg(target_os = "windows")]
    if let Err(eframe::Error::Wgpu(e)) = &result
        && let Some(app) = pending.take()
        && let Some(backends) = graphics::fallback_backends(backend_override, first_backends)
    {
        log::error!("graphics: the renderer didn't start on {first_backends:?} ({e}); trying {backends:?}");
        pending.set(Some(app));
        let mut options = retry_options;
        graphics::configure_backends(&mut options, backends);
        return run(options);
    }
    result
}
