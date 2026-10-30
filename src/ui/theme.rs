//! Design tokens for light and dark mode.

use std::sync::Arc;

use gpui_kit::{App, FontFeatures, Global, Hsla, Window, WindowAppearance, rgb};

use crate::settings::ThemePreference;

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
    /// Border of text inputs, selects and outline buttons: a step stronger
    /// than `border` so controls read as interactive.
    pub input_border: Hsla,
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
    /// Settings navigation: T3 Code's root `--sidebar`. In dark mode it is the
    /// card tone, a step above the true-black app sidebar on `surface_window`.
    pub settings_sidebar: Hsla,
    pub border_subtle: Hsla,
    pub border_hover: Hsla,
    /// Amber tones used by the "data retention disabled" banner.
    pub amber_border: Hsla,
    pub amber_bg: Hsla,
    pub amber_text: Hsla,
    pub amber_icon: Hsla,
    /// Opaque dark HUD surface behind the recording pill's hints.
    pub toast_surface: Hsla,
    /// White in light mode and dark mode alike; used for subtle overlays.
    pub white: Hsla,
}

fn c(hex: u32) -> Hsla {
    Hsla::from(rgb(hex))
}

impl Palette {
    /// T3 Code's light theme: a zinc-25 canvas beside a zinc-50 sidebar, white
    /// raised surfaces, zinc-200 borders and a blue-700 accent.
    pub fn light() -> Self {
        Self {
            dark: false,
            primary: c(0x1b4ed8),
            primary_foreground: c(0xffffff),
            muted: c(0xf4f4f5),
            muted_foreground: c(0x71717b),
            destructive: c(0xfb2c36),
            border: c(0xe4e4e7),
            input: c(0xffffff),
            input_border: c(0xd4d4d8),
            ring: c(0x1b4ed8),
            background: c(0xfcfcfc),
            foreground: c(0x18181b),
            card: c(0xffffff),
            popover: c(0xffffff),
            success: c(0x007a55),
            warning: c(0xbb4d00),
            surface_0: c(0xfcfcfc),
            surface_1: c(0xfafafa),
            surface_2: c(0xffffff),
            surface_3: c(0xf4f4f5),
            surface_raised: c(0xebebed),
            surface_window: c(0xfafafa),
            settings_sidebar: c(0xfafafa),
            border_subtle: c(0xe4e4e7),
            border_hover: c(0xd4d4d8),
            amber_border: c(0xfe9a00).opacity(0.3),
            amber_bg: c(0xfe9a00).opacity(0.08),
            amber_text: c(0xbb4d00),
            amber_icon: c(0xfe9a00),
            toast_surface: c(0x0a0b0e),
            white: c(0xffffff),
        }
    }

    /// T3 Code's dark theme: a true black sidebar and canvas split by a
    /// hairline, raised surfaces a step above neutral-950, translucent white
    /// borders and a brighter blue accent.
    pub fn dark() -> Self {
        Self {
            dark: true,
            primary: c(0x346bf1),
            primary_foreground: c(0xffffff),
            muted: c(0x141414),
            muted_foreground: c(0xa1a1a1),
            destructive: c(0xfb414a),
            border: c(0xffffff).opacity(0.08),
            input: c(0x0a0a0a),
            input_border: c(0xffffff).opacity(0.11),
            ring: c(0x346bf1),
            background: c(0x000000),
            foreground: c(0xf5f5f5),
            card: c(0x111111),
            popover: c(0x111111),
            success: c(0x00d492),
            warning: c(0xffb900),
            surface_0: c(0x000000),
            surface_1: c(0x0a0a0a),
            surface_2: c(0x111111),
            surface_3: c(0x141414),
            surface_raised: c(0x1e1e1e),
            surface_window: c(0x000000),
            settings_sidebar: c(0x111111),
            border_subtle: c(0xffffff).opacity(0.06),
            border_hover: c(0xffffff).opacity(0.11),
            amber_border: c(0xfe9a00).opacity(0.3),
            amber_bg: c(0xfe9a00).opacity(0.16),
            amber_text: c(0xffb900),
            amber_icon: c(0xfe9a00),
            toast_surface: c(0x0a0b0e),
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
    /// Whether `palette` is the dark one.
    pub dark: bool,
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
    let palette = if dark {
        Palette::dark()
    } else {
        Palette::light()
    };
    cx.set_global(Theme {
        preference,
        dark,
        palette,
    });
    sync_component_theme(&palette, cx);
}

/// GPUI Kit keeps its own theme, which stays on its light default and paints
/// the input caret near-black. Point the caret at this palette's text color.
fn sync_component_theme(palette: &Palette, cx: &mut App) {
    let caret = palette.foreground;
    gpui_kit::component::Theme::update(cx, |theme| theme.caret = caret);
}

/// Re-resolves the palette after a preference or system appearance change.
pub fn apply(preference: ThemePreference, cx: &mut App) {
    let dark = resolve_dark(preference, cx.window_appearance());
    let theme = cx.global_mut::<Theme>();
    theme.preference = preference;
    theme.dark = dark;
    theme.palette = if dark {
        Palette::dark()
    } else {
        Palette::light()
    };
    let palette = theme.palette;
    sync_component_theme(&palette, cx);
    cx.refresh_windows();
}

/// Keeps an Auto theme in step with the system appearance while `window`
/// lives. Every app window registers, so a change lands even when the
/// control panel is closed.
pub fn follow_system_appearance(window: &Window) {
    window
        .observe_window_appearance(|_, cx| {
            let theme = cx.global::<Theme>();
            let preference = theme.preference;
            let dark = resolve_dark(preference, cx.window_appearance());
            if preference == ThemePreference::Auto && dark != theme.dark {
                apply(preference, cx);
            }
        })
        .detach();
}

pub trait PaletteExt {
    fn palette(&self) -> Palette;
}

impl PaletteExt for App {
    fn palette(&self) -> Palette {
        self.global::<Theme>().palette
    }
}

/// Font families. The system face is the chrome and the user's voice; mono is
/// for code-like values.
///
/// - `FONT_UI`: navigation, labels, buttons, inputs, figures
/// - `FONT_VOICE`: what the user said: transcripts, raw transcripts and the
///   upload hero
/// - `FONT_MONO`: code-like values (URLs, API keys, model ids, prompts, key
///   chips, error details)
///
/// macOS maps the system UI family to SF Pro. JetBrains Mono Nerd Font Mono is
/// bundled with the app. CJK text falls back to PingFang in both families.
pub const FONT_UI: &str = ".AppleSystemUIFont";
pub const FONT_VOICE: &str = ".AppleSystemUIFont";
pub const FONT_MONO: &str = "JetBrainsMono Nerd Font Mono";

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
pub const TEXT_SMALL: f32 = 12.;
pub const TEXT_BODY: f32 = 14.;
pub const TEXT_READING: f32 = 15.5;
pub const TEXT_TITLE: f32 = 16.;
pub const TEXT_DISPLAY: f32 = 24.;
pub const TEXT_FIGURE: f32 = 34.;

/// Line heights paired with the scale.
pub const LINE_SMALL: f32 = 18.;
pub const LINE_READING: f32 = 25.;
pub const LINE_TITLE: f32 = 24.;

/// Workspace geometry. These keep the top bar, sidebar rows and settings
/// navigation on the same compact control rhythm.
/// The top bar centers on the traffic lights.
pub const WORKSPACE_TOP_BAR_HEIGHT: f32 = 52.;
pub const NAV_ROW_HEIGHT: f32 = 32.;
/// Height of buttons, inputs and selects at their default size.
pub const CONTROL_HEIGHT: f32 = 32.;

/// Corner radii. Every rounded surface picks one of these steps.
///
/// - `RADIUS_SM`: badges, menu items, small chips
/// - `RADIUS_MD`: compact controls, tooltips, segmented items
/// - `RADIUS_LG`: buttons, inputs, nav rows, menus, toasts
/// - `RADIUS_XL`: cards, settings groups, textareas
/// - `RADIUS_2XL`: dialogs, the settings sheet, the command palette
pub const RADIUS_SM: f32 = 4.;
pub const RADIUS_MD: f32 = 6.;
pub const RADIUS_LG: f32 = 8.;
pub const RADIUS_XL: f32 = 12.;
pub const RADIUS_2XL: f32 = 16.;

/// Tabular figures, so counts, times and durations line up and don't jitter.
pub fn tabular_nums() -> FontFeatures {
    FontFeatures(Arc::new(vec![("tnum".into(), 1)]))
}

/// A comfortable long-form reading measure. The canvas may be wider, but
/// transcripts and their metadata stay in one scan-friendly column.
pub const READING_MAX_WIDTH: f32 = 736.;

/// Motion durations, in ms. Five steps replace one-off timings:
///
/// - `FAST`: exits and press-in
/// - `SNAP`: hover reveals, menus and quick fades
/// - `BASE`: enters, reveals and view changes
/// - `SHEET`: layout morphs such as the sidebar
/// - `SLOW`: the press spring settling
pub const DUR_FAST: u64 = 60;
pub const DUR_SNAP: u64 = 120;
pub const DUR_BASE: u64 = 200;
pub const DUR_SHEET: u64 = 300;
pub const DUR_SLOW: u64 = 450;

/// Strong ease-out shared by enter motion.
pub fn ease_out(t: f32) -> f32 {
    cubic_bezier(0.165, 0.84, 0.44, 1.0, t)
}

/// `cubic-bezier(0.32, 0.72, 0, 1)`: quick settles for exits, menus and reveals.
pub fn ease_snap(t: f32) -> f32 {
    cubic_bezier(0.32, 0.72, 0.0, 1.0, t)
}

/// `cubic-bezier(0.34, 1.3, 0.64, 1)`: overshoots about 6% before settling.
pub fn ease_overshoot(t: f32) -> f32 {
    cubic_bezier(0.34, 1.3, 0.64, 1.0, t)
}

/// The button release spring, sampled into 15 evenly spaced stops and
/// interpolated linearly, like CSS `linear()`.
pub fn spring(t: f32) -> f32 {
    const STOPS: [f32; 15] = [
        0., 0.2459, 0.6526, 0.9468, 1.0764, 1.0915, 1.0585, 1.0219, 0.9993, 0.9914, 0.9921, 0.9957,
        0.9988, 1.0004, 1.,
    ];
    if t <= 0. {
        return 0.;
    }
    if t >= 1. {
        return 1.;
    }
    let x = t * (STOPS.len() - 1) as f32;
    let i = x.floor() as usize;
    STOPS[i] + (STOPS[i + 1] - STOPS[i]) * (x - i as f32)
}

/// Holds at 0 for `delay_ms`, then runs `ease` over `duration_ms`. For
/// staggering a `with_animation` that lasts `delay_ms + duration_ms`.
pub fn delayed(delay_ms: u64, duration_ms: u64, ease: fn(f32) -> f32) -> impl Fn(f32) -> f32 {
    let total = (delay_ms + duration_ms) as f32;
    let start = delay_ms as f32 / total;
    move |t| ease(((t - start) / (1. - start)).clamp(0., 1.))
}

/// Progress of a one-shot animation that started `elapsed` ago.
pub fn progress(elapsed: std::time::Duration, duration_ms: u64) -> f32 {
    (elapsed.as_secs_f32() * 1000. / duration_ms as f32).clamp(0., 1.)
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
            let v = ease_snap(i as f32 / 20.0);
            assert!(v >= last - 1e-4);
            last = v;
        }
        // Strong ease-out is well past half way at a quarter of the time.
        assert!(ease_out(0.25) > 0.6);
    }

    #[test]
    fn overshoot_and_spring_pass_the_target_then_settle() {
        let peak = (1..20)
            .map(|i| ease_overshoot(i as f32 / 20.))
            .fold(0., f32::max);
        assert!(peak > 1.02 && peak < 1.1, "{peak}");
        assert_eq!(spring(0.), 0.);
        assert_eq!(spring(1.), 1.);
        assert!((spring(5. / 14.) - 1.0915).abs() < 1e-4);
        assert!(spring(0.5) > 1.);
    }

    #[test]
    fn delayed_holds_then_runs() {
        let f = delayed(100, 200, ease_out);
        assert_eq!(f(0.), 0.);
        assert_eq!(f(1. / 3.), 0.);
        assert_eq!(f(1.), 1.);
        assert!(f(2. / 3.) > 0.5);
    }
}
