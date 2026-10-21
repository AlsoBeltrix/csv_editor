//! System fonts, so data in any script renders: the platform's interface font first, egui's
//! bundled fonts next, then symbol and CJK faces as fallbacks. Nothing is bundled; a machine
//! without a face keeps egui's defaults for it.
//!
//! The candidate lists follow GridCraft's `theme.rs` (MIT, see `ATTRIBUTION.md`). Every file is
//! checked with the same parser epaint uses before egui sees it: epaint panics on a font it
//! cannot read, and a font file is input like any other.

use std::sync::Arc;

use egui::{FontData, FontDefinitions, FontFamily};

/// One face to try: file candidates in preference order, each with its index in a collection.
struct Role {
    key: &'static str,
    /// Goes in front of egui's fonts (the interface font) or behind them (a fallback).
    leading: bool,
    candidates: Vec<(String, u32)>,
}

fn roles() -> Vec<Role> {
    let win = std::env::var("WINDIR").map(|w| format!("{w}\\Fonts")).unwrap_or_else(|_| "C:\\Windows\\Fonts".to_string());
    let mac = "/System/Library/Fonts";
    let lin = ["/usr/share/fonts/truetype", "/usr/share/fonts", "/usr/share/fonts/opentype", "/usr/local/share/fonts"];
    let mut ui = vec![(format!("{win}\\segoeui.ttf"), 0), (format!("{mac}/SFNS.ttf"), 0), (format!("{mac}/HelveticaNeue.ttc"), 0)];
    let mut symbols = vec![(format!("{win}\\seguisym.ttf"), 0), (format!("{mac}/Apple Symbols.ttf"), 0)];
    let mut cjk_sc = vec![(format!("{win}\\msyh.ttc"), 0), (format!("{mac}/PingFang.ttc"), 0)];
    let mut cjk_jp = vec![(format!("{win}\\YuGothR.ttc"), 0), (format!("{mac}/ヒラギノ角ゴシック W3.ttc"), 0)];
    let mut cjk_kr = vec![(format!("{win}\\malgun.ttf"), 0), (format!("{mac}/AppleSDGothicNeo.ttc"), 0)];
    for d in lin {
        ui.push((format!("{d}/dejavu/DejaVuSans.ttf"), 0));
        ui.push((format!("{d}/noto/NotoSans-Regular.ttf"), 0));
        symbols.push((format!("{d}/dejavu/DejaVuSans.ttf"), 0));
        cjk_sc.push((format!("{d}/noto/NotoSansCJK-Regular.ttc"), 2));
        cjk_jp.push((format!("{d}/noto/NotoSansCJK-Regular.ttc"), 0));
        cjk_kr.push((format!("{d}/noto/NotoSansCJK-Regular.ttc"), 1));
    }
    vec![
        Role { key: "sys-ui", leading: true, candidates: ui },
        Role { key: "sys-symbols", leading: false, candidates: symbols },
        Role { key: "sys-cjk-sc", leading: false, candidates: cjk_sc },
        Role { key: "sys-cjk-jp", leading: false, candidates: cjk_jp },
        Role { key: "sys-cjk-kr", leading: false, candidates: cjk_kr },
    ]
}

/// Whether epaint can open face `index` of this font file and it maps characters to glyphs.
pub(crate) fn font_parses(bytes: &[u8], index: u32) -> bool {
    use skrifa::MetadataProvider;
    skrifa::FontRef::from_index(bytes, index).is_ok_and(|font| font.charmap().has_map())
}

/// The first candidate that exists and parses.
fn load(candidates: &[(String, u32)]) -> Option<FontData> {
    for (path, index) in candidates {
        let Ok(bytes) = std::fs::read(path) else { continue };
        if !font_parses(&bytes, *index) {
            log::warn!("skipping font {path} (face {index}): not a font file egui can read");
            continue;
        }
        let mut data = FontData::from_owned(bytes);
        data.index = *index;
        return Some(data);
    }
    None
}

/// Builds the font set: system faces around egui's defaults, as described at the top.
pub fn definitions() -> FontDefinitions {
    let mut defs = FontDefinitions::default();
    let mut leading = Vec::new();
    let mut trailing = Vec::new();
    for role in roles() {
        if let Some(data) = load(&role.candidates) {
            defs.font_data.insert(role.key.to_string(), Arc::new(data));
            if role.leading { leading.push(role.key.to_string()) } else { trailing.push(role.key.to_string()) }
        }
    }
    for family in [FontFamily::Proportional, FontFamily::Monospace] {
        let chain = defs.families.entry(family.clone()).or_default();
        if family == FontFamily::Proportional {
            for (i, key) in leading.iter().enumerate() {
                chain.insert(i, key.clone());
            }
        }
        chain.extend(trailing.iter().cloned());
    }
    log::info!("fonts: {} system face(s) loaded", leading.len() + trailing.len());
    defs
}

/// Installs [`definitions`] into the context. Call once at start.
pub fn install(ctx: &egui::Context) {
    ctx.set_fonts(definitions());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn broken_files_are_rejected_not_fatal() {
        assert!(!font_parses(b"not a font", 0));
        assert!(!font_parses(&[], 0));
        let dir = std::env::temp_dir().join(format!("csv-editor-fonts-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let bad = dir.join("bad.ttf");
        std::fs::write(&bad, b"\0\x01\0\0junk").unwrap();
        assert!(load(&[(bad.display().to_string(), 0), ("/nonexistent/font.ttf".to_string(), 0)]).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn definitions_keep_egui_defaults_and_stay_consistent() {
        let defs = definitions();
        for (family, chain) in &defs.families {
            assert!(!chain.is_empty(), "{family:?} has no fonts");
            for key in chain {
                assert!(defs.font_data.contains_key(key), "{family:?} names {key} which has no data");
            }
        }
    }
}
