//! CSV editor desktop app.
//!
//! Usage: `csv-editor [files…]`
#![cfg_attr(all(target_os = "windows", not(debug_assertions)), windows_subsystem = "windows")]

mod graphics;
mod prefs;

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use csv_editor_ui::{EditorApp, Services};

struct App {
    editor: EditorApp,
    frames: u32,
    /// The window's inner size, remembered for the next start.
    window: Option<[f32; 2]>,
    /// `CSV_EDITOR_SCREENSHOT=<file.png>`: save the window after a few frames and quit. A
    /// development aid for looking at the interface without a person at the screen.
    screenshot: Option<PathBuf>,
    /// A screenshot was requested and its callback has not run yet. The read-back completes on
    /// a later frame on some backends (OpenGL), so frames keep coming until it does.
    screenshot_pending: Arc<AtomicBool>,
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.editor.logic(ctx);
        self.frames = self.frames.saturating_add(1);
        if let Some(rect) = ctx.input(|i| i.viewport().inner_rect)
            && rect.width() > 100.0
            && rect.height() > 100.0
        {
            self.window = Some([rect.width(), rect.height()]);
        }
        if self.frames == 4
            && let Some(path) = self.screenshot.take()
        {
            let done = ctx.clone();
            let pending = Arc::clone(&self.screenshot_pending);
            pending.store(true, Ordering::Relaxed);
            ctx.request_screenshot(move |img| {
                let (w, h) = (img.width() as u32, img.height() as u32);
                match image::save_buffer(&path, img.as_raw(), w, h, image::ColorType::Rgba8) {
                    Ok(()) => log::info!("screenshot saved to {}", path.display()),
                    Err(e) => log::error!("screenshot {}: {e}", path.display()),
                }
                pending.store(false, Ordering::Relaxed);
                done.send_viewport_cmd(egui::ViewportCommand::Close);
            });
        }
        if self.screenshot.is_some() || self.screenshot_pending.load(Ordering::Relaxed) {
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

    fn on_exit(&mut self) {
        prefs::save(&prefs::Prefs {
            dark: self.editor.dark,
            system_theme: self.editor.system_theme,
            zoom: self.editor.grid.zoom,
            recent: self.editor.recent.clone(),
            window: self.window,
        });
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
    let saved = prefs::load();
    let mut app = EditorApp::new(services());
    app.dark = saved.dark;
    app.system_theme = saved.system_theme;
    app.grid.set_zoom(saved.zoom);
    app.recent = saved.recent.into_iter().filter(|p| p.exists()).collect();
    for f in &files {
        app.open_path(f);
    }
    let size = saved.window.filter(|[w, h]| w.is_finite() && h.is_finite() && *w >= 560.0 && *h >= 360.0).unwrap_or([1280.0, 800.0]);
    let viewport = egui::ViewportBuilder::default()
        .with_title("CSV Editor")
        .with_inner_size(size)
        .with_min_inner_size([560.0, 360.0])
        .with_app_id("csv-editor")
        .with_drag_and_drop(true);
    let mut options = eframe::NativeOptions { viewport, ..Default::default() };
    // Before eframe creates the wgpu instance: OpenGL on Windows, never DirectX or Vulkan unless
    // asked (see graphics.rs).
    match graphics::configure(&mut options, eframe::wgpu::Backends::from_env()) {
        Some(backends) => log::info!("graphics: backends {backends:?}"),
        None => log::info!("graphics: wgpu's default backends"),
    }
    let screenshot = std::env::var_os("CSV_EDITOR_SCREENSHOT").map(PathBuf::from);
    eframe::run_native(
        "csv-editor",
        options,
        Box::new(move |_cc| {
            Ok(Box::new(App { editor: app, frames: 0, window: None, screenshot, screenshot_pending: Arc::new(AtomicBool::new(false)) }))
        }),
    )
}
