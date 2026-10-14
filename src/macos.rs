//! AppKit calls GPUI does not expose: click-through, ordering and moving
//! the borderless recording-pill window.

use gpui_kit::{Bounds, Pixels, Point, Window, point, px};
use objc2::rc::Retained;
use objc2_app_kit::{NSApplication, NSEvent, NSScreen, NSView, NSWindow, NSWindowStyleMask};
use objc2_foundation::{MainThreadMarker, NSPoint};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

fn ns_window(window: &Window) -> Option<Retained<NSWindow>> {
    let handle = HasWindowHandle::window_handle(window).ok()?;
    let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
        return None;
    };
    // SAFETY: GPUI hands out its live content view; we only borrow it.
    let view: &NSView = unsafe { appkit.ns_view.cast::<NSView>().as_ref() };
    view.window()
}

/// Lets clicks fall through to the app underneath while the pointer is not
/// over the pill or one of its panels.
pub fn set_ignores_mouse_events(window: &Window, ignore: bool) {
    if let Some(w) = ns_window(window)
        && w.ignoresMouseEvents() != ignore
    {
        w.setIgnoresMouseEvents(ignore);
    }
}

/// Drops the titled frame GPUI gives every window, so the transparent pill
/// window has no hairline border, rounded frame or shadow of its own.
pub fn make_borderless(window: &Window) {
    if let Some(w) = ns_window(window) {
        let keep = w.styleMask() & NSWindowStyleMask::NonactivatingPanel;
        w.setStyleMask(NSWindowStyleMask::Borderless | keep);
        w.setHasShadow(false);
    }
}

/// Shows the window without activating the app.
pub fn order_front(window: &Window) {
    if let Some(w) = ns_window(window) {
        w.orderFrontRegardless();
    }
}

pub fn order_out(window: &Window) {
    if let Some(w) = ns_window(window) {
        w.orderOut(None);
    }
}

/// The standard About panel.
pub fn show_about_panel() {
    if let Some(mtm) = MainThreadMarker::new() {
        NSApplication::sharedApplication(mtm).orderFrontStandardAboutPanel(None);
    }
}

fn main_screen_height() -> f64 {
    let Some(mtm) = MainThreadMarker::new() else {
        return 0.;
    };
    NSScreen::screens(mtm)
        .firstObject()
        .map(|s| s.frame().size.height)
        .unwrap_or(0.)
}

/// Moves the window so its top-left corner sits at `origin` in GPUI screen
/// coordinates (top-left origin, y down).
pub fn set_origin(window: &Window, origin: Point<Pixels>, height: Pixels) {
    if let Some(w) = ns_window(window) {
        let flipped_y = main_screen_height() - f64::from(origin.y) - f64::from(height);
        w.setFrameOrigin(NSPoint::new(f64::from(origin.x), flipped_y));
    }
}

/// The pointer in GPUI screen coordinates (top-left origin, y down).
pub fn mouse_location() -> Point<Pixels> {
    let location = NSEvent::mouseLocation();
    point(
        px(location.x as f32),
        px((main_screen_height() - location.y) as f32),
    )
}

/// The window's frame in GPUI screen coordinates.
pub fn window_frame(window: &Window) -> Option<Bounds<Pixels>> {
    let w = ns_window(window)?;
    let frame = w.frame();
    let top = main_screen_height() - frame.origin.y - frame.size.height;
    Some(Bounds::new(
        point(px(frame.origin.x as f32), px(top as f32)),
        gpui_kit::size(px(frame.size.width as f32), px(frame.size.height as f32)),
    ))
}
