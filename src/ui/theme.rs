//! Design tokens for light and dark mode.

use gpui_kit::{App, Global, Hsla, WindowAppearance, rgb};

use whisper::settings::ThemePreference;

#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub dark: bool,
    pub primary: Hsla,
    pub primary_foreground: Hsla,
    pub muted: Hsla,
    pub muted_foreground: Hsla,
    pub destructive: Hsla,
    pub border: Hsla,
    pub input: Hsla,
    pub ring: Hsla,
    pub background: Hsla,
    pub foreground: Hsla,
    pub card: Hsla,
    pub popover: Hsla,
    pub success: Hsla,
    pub warning: Hsla,
    pub surface_0: Hsla,
    pub surface_1: Hsla,
    pub surface_2: Hsla,
    pub surface_3: Hsla,
    pub surface_raised: Hsla,
    pub surface_window: Hsla,
    pub border_subtle: Hsla,
    pub border_hover: Hsla,
    /// Amber tones used by the "data retention disabled" banner.
    pub amber_border: Hsla,
    pub amber_bg: Hsla,
    pub amber_text: Hsla,
    pub amber_icon: Hsla,
    /// Opaque dark HUD surface shared by light and dark toasts.
    pub toast_surface: Hsla,
    pub toast_border: Hsla,
    /// White in light mode and dark mode alike; used for subtle overlays.
    pub white: Hsla,
}

fn c(hex: u32) -> Hsla {
    Hsla::from(rgb(hex))
}

impl Palette {
    pub fn light() -> Self {
        Self {
            dark: false,
            primary: c(0x4577e9),
            primary_foreground: c(0xffffff),
            muted: c(0xf5f5f5),
            muted_foreground: c(0x525252),
            destructive: c(0xdc2626),
            border: c(0xe5e5e5),
            input: c(0xf9f6f1),
            ring: c(0x4577e9),
            background: c(0xffffff),
            foreground: c(0x171717),
            card: c(0xffffff),
            popover: c(0xffffff),
            success: c(0x16a34a),
            warning: c(0xd97706),
            surface_0: c(0xffffff),
            surface_1: c(0xfafafa),
            surface_2: c(0xffffff),
            surface_3: c(0xf5f5f5),
            surface_raised: c(0xf0f0f0),
            surface_window: c(0xf7f7f7),
            border_subtle: c(0xe5e5e5),
            border_hover: c(0xd4d4d4),
            amber_border: c(0xf59e0b).opacity(0.3),
            amber_bg: c(0xf59e0b).opacity(0.05),
            amber_text: c(0xb45309),
            amber_icon: c(0xd97706),
            toast_surface: c(0x0a0b0e),
            toast_border: c(0x202225),
            white: c(0xffffff),
        }
    }

    pub fn dark() -> Self {
        Self {
            dark: true,
            primary: c(0xededed),
            primary_foreground: c(0x111111),
            muted: c(0x1d1d1d),
            muted_foreground: c(0x8b8b8b),
            destructive: c(0xee343b),
            border: c(0x262626),
            input: c(0x121212),
            ring: c(0x888888),
            background: c(0x161616),
            foreground: c(0xececec),
            card: c(0x1b1b1b),
            popover: c(0x1d1d1d),
            success: c(0x45ba50),
            warning: c(0xe99b2a),
            surface_0: c(0x161616),
            surface_1: c(0x191919),
            surface_2: c(0x1c1c1c),
            surface_3: c(0x222222),
            surface_raised: c(0x2a2a2a),
            surface_window: c(0x0f0f0f),
            border_subtle: c(0x222222),
            border_hover: c(0x333333),
            amber_border: c(0xf59e0b).opacity(0.3),
            amber_bg: c(0xf59e0b).opacity(0.1),
            amber_text: c(0xfcd34d).opacity(0.9),
            amber_icon: c(0xfbbf24),
            toast_surface: c(0x0a0b0e),
            toast_border: c(0x202225),
            white: c(0xffffff),
        }
    }

    /// Hover wash: foreground in light mode, white in dark mode, at `alpha`.
    pub fn hover_wash(&self, alpha: f32) -> Hsla {
        if self.dark {
            self.white.opacity(alpha)
        } else {
            self.foreground.opacity(alpha)
        }
    }
}

/// The resolved theme, kept as a GPUI global.
pub struct Theme {
    pub preference: ThemePreference,
    pub palette: Palette,
}

impl Global for Theme {}

pub fn resolve_dark(preference: ThemePreference, appearance: WindowAppearance) -> bool {
    match preference {
        ThemePreference::Light => false,
        ThemePreference::Dark => true,
        ThemePreference::Auto => matches!(
            appearance,
            WindowAppearance::Dark | WindowAppearance::VibrantDark
        ),
    }
}

pub fn init(preference: ThemePreference, cx: &mut App) {
    let dark = resolve_dark(preference, cx.window_appearance());
    cx.set_global(Theme {
        preference,
        palette: if dark {
            Palette::dark()
        } else {
            Palette::light()
        },
    });
}

/// Re-resolves the palette after a preference or system appearance change.
pub fn apply(preference: ThemePreference, cx: &mut App) {
    let dark = resolve_dark(preference, cx.window_appearance());
    let theme = cx.global_mut::<Theme>();
    theme.preference = preference;
    theme.palette = if dark {
        Palette::dark()
    } else {
        Palette::light()
    };
    cx.refresh_windows();
}

pub trait PaletteExt {
    fn palette(&self) -> Palette;
}

impl PaletteExt for App {
    fn palette(&self) -> Palette {
        self.global::<Theme>().palette
    }
}

/// Font families. Sans is the chrome, serif is the user's voice, mono is code.
///
/// - `FONT_UI`: navigation, labels, buttons, inputs, figures
/// - `FONT_VOICE`: what the user said: transcripts, raw transcripts and the
///   upload hero
/// - `FONT_MONO`: code-like values (URLs, API keys, model ids, prompts, key
///   chips, error details)
///
/// Anthropic Sans and Serif are static Text-size cuts made by
/// `scripts/instance-anthropic-fonts.py`; JetBrains Mono is bundled as-is. CJK
/// text falls back to the system face (PingFang) in every family.
pub const FONT_UI: &str = "Anthropic Sans";
pub const FONT_VOICE: &str = "Anthropic Serif";
pub const FONT_MONO: &str = "JetBrains Mono";

/// Type scale shared by every window, in px. Reading text runs larger than
/// body text: transcripts are a reading surface, the rest is controls.
///
/// - `CAPTION`: group labels, badges, key chips, timestamps, fine print
/// - `SMALL`: descriptions, helper text, metadata, small controls
/// - `BODY`: labels, navigation, buttons, inputs, list rows
/// - `READING`: transcripts and other long-form content
/// - `TITLE`: page, section and dialog titles
/// - `DISPLAY`: hero titles
/// - `FIGURE`: large numbers
pub const TEXT_CAPTION: f32 = 11.;
pub const TEXT_SMALL: f32 = 12.5;
pub const TEXT_BODY: f32 = 13.5;
pub const TEXT_READING: f32 = 15.5;
pub const TEXT_TITLE: f32 = 15.;
pub const TEXT_DISPLAY: f32 = 23.;
pub const TEXT_FIGURE: f32 = 34.;

/// Line heights paired with the scale.
pub const LINE_SMALL: f32 = 19.;
pub const LINE_READING: f32 = 25.;
pub const LINE_TITLE: f32 = 22.;

/// Strong ease-out shared by enter/exit motion.
pub fn ease_out(t: f32) -> f32 {
    cubic_bezier(0.23, 1.0, 0.32, 1.0, t)
}

/// `cubic-bezier(0.2, 0, 0, 1)`: the pill's footprint morph.
pub fn ease_grow(t: f32) -> f32 {
    cubic_bezier(0.2, 0.0, 0.0, 1.0, t)
}

/// The standard `ease` curve.
pub fn ease_css(t: f32) -> f32 {
    cubic_bezier(0.25, 0.1, 0.25, 1.0, t)
}

/// Evaluates a cubic-bezier timing function at progress `t`.
pub fn cubic_bezier(x1: f32, y1: f32, x2: f32, y2: f32, t: f32) -> f32 {
    if t <= 0.0 {
        return 0.0;
    }
    if t >= 1.0 {
        return 1.0;
    }
    let bez = |a: f32, b: f32, s: f32| {
        let inv = 1.0 - s;
        3.0 * inv * inv * s * a + 3.0 * inv * s * s * b + s * s * s
    };
    // Solve x(s) = t with Newton steps, falling back to bisection.
    let mut s = t;
    for _ in 0..8 {
        let x = bez(x1, x2, s) - t;
        let inv = 1.0 - s;
        let dx = 3.0 * inv * inv * x1 + 6.0 * inv * s * (x2 - x1) + 3.0 * s * s * (1.0 - x2);
        if dx.abs() < 1e-6 {
            break;
        }
        s -= x / dx;
    }
    if !(0.0..=1.0).contains(&s) || (bez(x1, x2, s) - t).abs() > 1e-3 {
        let (mut lo, mut hi) = (0.0f32, 1.0f32);
        s = t;
        for _ in 0..30 {
            let x = bez(x1, x2, s);
            if (x - t).abs() < 1e-5 {
                break;
            }
            if x < t {
                lo = s;
            } else {
                hi = s;
            }
            s = (lo + hi) / 2.0;
        }
    }
    bez(y1, y2, s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bezier_endpoints_and_monotonic() {
        assert_eq!(ease_out(0.0), 0.0);
        assert_eq!(ease_out(1.0), 1.0);
        let mut last = 0.0;
        for i in 1..=20 {
            let v = ease_grow(i as f32 / 20.0);
            assert!(v >= last - 1e-4);
            last = v;
        }
        // Strong ease-out is well past half way at a quarter of the time.
        assert!(ease_out(0.25) > 0.6);
    }
}
