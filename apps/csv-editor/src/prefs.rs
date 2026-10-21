//! Preferences on disk: theme, zoom, recent files and the window size, as JSON in the user's
//! settings folder. Anything unreadable falls back to the defaults; nothing here can fail loudly.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    pub dark: bool,
    pub system_theme: bool,
    pub zoom: f32,
    pub recent: Vec<PathBuf>,
    /// Inner window size in points.
    pub window: Option<[f32; 2]>,
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs { dark: false, system_theme: true, zoom: 1.0, recent: Vec::new(), window: None }
    }
}

/// The settings folder: `%APPDATA%\csv-editor` on Windows, `~/Library/Application Support/csv-editor`
/// on macOS, `$XDG_CONFIG_HOME/csv-editor` (or `~/.config/csv-editor`) elsewhere.
pub fn config_dir() -> Option<PathBuf> {
    if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support/csv-editor"))
    } else if cfg!(windows) {
        std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("csv-editor"))
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
            .map(|c| c.join("csv-editor"))
    }
}

/// `CSV_EDITOR_NO_PREFS` (and screenshot runs) neither read nor write preferences.
pub fn enabled() -> bool {
    std::env::var_os("CSV_EDITOR_NO_PREFS").is_none() && std::env::var_os("CSV_EDITOR_SCREENSHOT").is_none()
}

pub fn load() -> Prefs {
    if !enabled() {
        return Prefs::default();
    }
    let Some(dir) = config_dir() else { return Prefs::default() };
    match std::fs::read(dir.join("prefs.json")) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|e| {
            log::warn!("prefs.json is not readable ({e}); using defaults");
            Prefs::default()
        }),
        Err(_) => Prefs::default(),
    }
}

pub fn save(prefs: &Prefs) {
    if !enabled() {
        return;
    }
    let Some(dir) = config_dir() else { return };
    if let Err(e) = std::fs::create_dir_all(&dir) {
        log::warn!("cannot create {}: {e}", dir.display());
        return;
    }
    match serde_json::to_vec_pretty(prefs) {
        Ok(bytes) => {
            if let Err(e) = std::fs::write(dir.join("prefs.json"), bytes) {
                log::warn!("cannot write prefs.json: {e}");
            }
        }
        Err(e) => log::warn!("cannot encode preferences: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_and_missing_fields_fall_back_to_defaults() {
        let p: Prefs = serde_json::from_str(r#"{"zoom": 1.5, "future": true}"#).unwrap();
        assert_eq!(p.zoom, 1.5);
        assert!(p.system_theme && p.recent.is_empty() && p.window.is_none());
        let round: Prefs = serde_json::from_slice(&serde_json::to_vec(&p).unwrap()).unwrap();
        assert_eq!(round.zoom, 1.5);
    }
}
