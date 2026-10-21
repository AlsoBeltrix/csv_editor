//! CSV editor desktop app.
//!
//! Usage: `csv-editor [files…]`
#![cfg_attr(all(target_os = "windows", not(debug_assertions)), windows_subsystem = "windows")]

#[cfg(any(target_os = "windows", test))]
mod graphics;

use std::path::PathBuf;

use csv_editor_ui::{EditorApp, Services};

struct App {
    editor: EditorApp,
    frames: u32,
    /// `CSV_EDITOR_SCREENSHOT=<file.png>`: save the window after a few frames and quit. A
    /// development aid for looking at the interface without a person at the screen.
    screenshot: Option<PathBuf>,
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.editor.logic(ctx);
        self.frames = self.frames.saturating_add(1);
        if self.frames == 4
            && let Some(path) = self.screenshot.take()
        {
            let done = ctx.clone();
            ctx.request_screenshot(move |img| {
                let (w, h) = (img.width() as u32, img.height() as u32);
                match image::save_buffer(&path, img.as_raw(), w, h, image::ColorType::Rgba8) {
                    Ok(()) => log::info!("screenshot saved to {}", path.display()),
                    Err(e) => log::error!("screenshot {}: {e}", path.display()),
                }
                done.send_viewport_cmd(egui::ViewportCommand::Close);
            });
        }
        if self.screenshot.is_some() {
            ctx.request_repaint();
        }
    }

    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw: &mut egui::RawInput) {
        // Files dropped on the window open in new tabs.
        for f in std::mem::take(&mut raw.dropped_files) {
            let path = f.path().to_path_buf();
            if !path.as_os_str().is_empty() {
                self.editor.open_path(&path);
            }
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.editor.ui(ui);
    }
}

fn services() -> Services {
    Services {
        pick_open: Some(Box::new(|| {
            rfd::FileDialog::new()
                .add_filter("Delimited text (CSV, TSV, TXT)", &["csv", "tsv", "txt", "tab"])
                .add_filter("All files", &["*"])
                .pick_files()
                .unwrap_or_default()
        })),
        pick_save: Some(Box::new(|suggested: &str| {
            rfd::FileDialog::new()
                .set_file_name(suggested)
                .add_filter("CSV (comma separated)", &["csv"])
                .add_filter("Tab separated", &["tsv", "txt"])
                .add_filter("All files", &["*"])
                .save_file()
        })),
        paste_text: Some(std::rc::Rc::new(|| arboard::Clipboard::new().ok()?.get_text().ok())),
    }
}

fn main() -> eframe::Result {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let files: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    let mut app = EditorApp::new(services());
    for f in &files {
        app.open_path(f);
    }
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
                Ok(Box::new(App { editor: app, frames: 0, screenshot: std::env::var_os("CSV_EDITOR_SCREENSHOT").map(PathBuf::from) }))
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
