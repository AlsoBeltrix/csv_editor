//! The application icon for the window and taskbar, embedded from `assets/icon/` (the source
//! is `assets/icon/icon.py`; the Windows executable's own icon comes from `build.rs`).

use std::sync::Arc;

use eframe::egui::IconData;

/// The 256 px rendering; the window system scales it to what the title bar and taskbar need.
const PNG: &[u8] = include_bytes!("../../../assets/icon/csv-editor-256.png");

/// The icon decoded for eframe, or `None` if the embedded image failed to decode (the window
/// then shows the system default rather than failing to open).
pub fn window_icon() -> Option<Arc<IconData>> {
    match eframe::icon_data::from_png_bytes(PNG) {
        Ok(icon) => Some(Arc::new(icon)),
        Err(e) => {
            log::error!("icon: embedded PNG did not decode: {e}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_embedded_icon_is_a_256_px_square() {
        let icon = window_icon().expect("the committed PNG decodes");
        assert_eq!((icon.width, icon.height), (256, 256));
        assert_eq!(icon.rgba.len(), 256 * 256 * 4);
        // Transparent corners (the page does not fill the square), opaque green in the middle.
        assert_eq!(icon.rgba[3], 0, "top-left pixel is transparent");
        let mid = (128 * 256 + 128) * 4;
        assert_eq!(icon.rgba[mid + 3], 255, "centre pixel is opaque");
    }
}
