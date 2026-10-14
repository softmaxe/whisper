//! Embedded icons and fonts, with GPUI Kit's own assets as a fallback for the
//! component library.

use std::borrow::Cow;

use anyhow::Result;
use gpui_kit::{App, AssetSource, SharedString, font};
use rust_embed::RustEmbed;

use super::theme::{FONT_MONO, FONT_UI, FONT_VOICE};

#[derive(RustEmbed)]
#[folder = "assets"]
#[include = "icons/*.svg"]
#[include = "fonts/*.ttf"]
#[include = "*.png"]
struct Embedded;

pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some(file) = Embedded::get(path) {
            return Ok(Some(file.data));
        }
        gpui_kit::assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut items: Vec<SharedString> = Embedded::iter()
            .filter(|p| p.starts_with(path))
            .map(|p| SharedString::from(p.to_string()))
            .collect();
        items.extend(gpui_kit::assets::Assets.list(path)?);
        Ok(items)
    }
}

/// Registers the bundled Anthropic Sans, Anthropic Serif and JetBrains Mono
/// faces, then checks that each family the UI asks for resolves to a bundled
/// face instead of the fallback stack.
pub fn load_fonts(cx: &mut App) {
    let fonts: Vec<Cow<'static, [u8]>> = Embedded::iter()
        .filter(|p| p.starts_with("fonts/") && p.ends_with(".ttf"))
        .filter_map(|p| Embedded::get(&p).map(|f| f.data))
        .collect();
    if let Err(err) = cx.text_system().add_fonts(fonts) {
        log::warn!("failed to load bundled fonts: {err}");
    }
    let text = cx.text_system();
    let missing: Vec<&str> = [FONT_UI, FONT_VOICE, FONT_MONO]
        .into_iter()
        .filter(|family| {
            let id = text.resolve_font(&font(*family));
            text.get_font_for_id(id)
                .is_none_or(|resolved| resolved.family.as_ref() != *family)
        })
        .collect();
    if missing.is_empty() {
        log::info!("bundled fonts resolved: {FONT_UI}, {FONT_VOICE}, {FONT_MONO}");
    } else {
        log::warn!("bundled fonts fell back for: {}", missing.join(", "));
    }
}

/// Raw bytes of an embedded asset.
pub fn bytes(path: &str) -> Option<Cow<'static, [u8]>> {
    Embedded::get(path).map(|f| f.data)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    /// (family, weight class, italic) of every embedded font file.
    fn faces() -> Vec<(String, u16, bool, Vec<u8>)> {
        Embedded::iter()
            .filter(|p| p.starts_with("fonts/") && p.ends_with(".ttf"))
            .map(|p| {
                let data = Embedded::get(&p).unwrap().data.into_owned();
                let face = ttf_parser::Face::parse(&data, 0).unwrap();
                let family = face
                    .names()
                    .into_iter()
                    .filter(|n| n.name_id == 16 || n.name_id == 1)
                    .filter_map(|n| Some((n.name_id != 16, n.to_string()?)))
                    .min()
                    .map(|(_, name)| name)
                    .unwrap();
                let weight = face.weight().to_number();
                let italic = face.is_italic();
                (family, weight, italic, data)
            })
            .collect()
    }

    #[test]
    fn every_role_has_the_faces_the_ui_uses() {
        let have: BTreeSet<(String, u16, bool)> =
            faces().into_iter().map(|(f, w, i, _)| (f, w, i)).collect();
        // Weights the views ask for: NORMAL, MEDIUM, SEMIBOLD, plus italic fine print.
        for (family, weight, italic) in [
            (FONT_UI, 400, false),
            (FONT_UI, 500, false),
            (FONT_UI, 600, false),
            (FONT_UI, 400, true),
            (FONT_VOICE, 400, false),
            (FONT_VOICE, 500, false),
            (FONT_VOICE, 400, true),
            (FONT_MONO, 400, false),
        ] {
            assert!(
                have.contains(&(family.to_string(), weight, italic)),
                "missing {family} {weight} italic={italic}; have {have:?}"
            );
        }
    }

    /// Every non-CJK character of the English and Chinese UI strings is drawn
    /// by Anthropic Sans itself, so chrome never mixes in a fallback Latin face.
    #[test]
    fn ui_face_covers_every_locale_character() {
        let faces = faces();
        let sans = faces
            .iter()
            .find(|(f, w, i, _)| f == FONT_UI && *w == 400 && !*i)
            .unwrap();
        let face = ttf_parser::Face::parse(&sans.3, 0).unwrap();
        let mut missing = BTreeSet::new();
        for file in ["en.json", "zh-CN.json"] {
            let path = concat!(env!("CARGO_MANIFEST_DIR"), "/locales/");
            let text = std::fs::read_to_string(format!("{path}{file}")).unwrap();
            let json: serde_json::Value = serde_json::from_str(&text).unwrap();
            let mut stack = vec![&json];
            while let Some(v) = stack.pop() {
                match v {
                    serde_json::Value::String(s) => {
                        for c in s.chars().filter(|c| !is_system_fallback(*c)) {
                            if face.glyph_index(c).is_none() {
                                missing.insert(c);
                            }
                        }
                    }
                    serde_json::Value::Object(m) => stack.extend(m.values()),
                    serde_json::Value::Array(a) => stack.extend(a.iter()),
                    _ => {}
                }
            }
        }
        assert!(missing.is_empty(), "{FONT_UI} lacks {missing:?}");
    }

    /// CJK, fullwidth punctuation, symbols and emoji are left to the system
    /// fallback (PingFang, Apple Symbols) by design.
    fn is_system_fallback(c: char) -> bool {
        c.is_control()
            || matches!(c as u32,
                0x2190..=0x23FF // arrows, technical symbols (⌘ ⌥ ⇧ ⏎)
                | 0x2460..=0x27BF // enclosed alphanumerics, shapes, dingbats
                | 0x2E80..=0x9FFF // CJK radicals, punctuation, kana, ideographs
                | 0xF900..=0xFAFF
                | 0xFE30..=0xFE4F
                | 0xFF00..=0xFFEF // fullwidth forms
                | 0x1F000..=0x1FFFF) // emoji
    }
}
