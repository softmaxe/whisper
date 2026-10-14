//! Embedded icons and fonts, with GPUI Kit's own assets as a fallback for the
//! component library.

use std::borrow::Cow;

use anyhow::Result;
use gpui_kit::{App, AssetSource, SharedString};
use rust_embed::RustEmbed;

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

/// Registers the bundled JetBrains Mono faces.
pub fn load_fonts(cx: &mut App) {
    let fonts: Vec<Cow<'static, [u8]>> = Embedded::iter()
        .filter(|p| p.starts_with("fonts/") && p.ends_with(".ttf"))
        .filter_map(|p| Embedded::get(&p).map(|f| f.data))
        .collect();
    if let Err(err) = cx.text_system().add_fonts(fonts) {
        log::warn!("failed to load bundled fonts: {err}");
    }
}

/// Raw bytes of an embedded asset.
pub fn bytes(path: &str) -> Option<Cow<'static, [u8]>> {
    Embedded::get(path).map(|f| f.data)
}
