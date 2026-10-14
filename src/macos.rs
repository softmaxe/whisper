//! AppKit calls GPUI does not expose: click-through, ordering and moving
//! the borderless recording-pill window, and the Reduce Motion setting.

use gpui_kit::{App, Bounds, Pixels, Point, Window, point, px};
use std::ptr::NonNull;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2_app_kit::{
    NSApplication, NSEvent, NSScreen, NSView, NSWindow, NSWindowStyleMask, NSWorkspace,
    NSWorkspaceAccessibilityDisplayOptionsDidChangeNotification,
};
use objc2_foundation::{MainThreadMarker, NSNotification, NSOperationQueue, NSPoint};
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

/// Runs an AppKit window change once the current GPUI update returns. Style
/// and frame changes make AppKit call GPUI's resize and move handlers
/// synchronously, and those need the app GPUI is still holding, so made
/// inside an update they fail with "RefCell already borrowed" and GPUI misses
/// the new bounds. Changes queued here run in order.
fn after_update(window: &Window, cx: &App, change: impl FnOnce(&NSWindow) + 'static) {
    if let Some(w) = ns_window(window) {
        cx.foreground_executor()
            .spawn(async move { change(&w) })
            .detach();
    }
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
pub fn make_borderless(window: &Window, cx: &App) {
    after_update(window, cx, |w| {
        let keep = w.styleMask() & NSWindowStyleMask::NonactivatingPanel;
        w.setStyleMask(NSWindowStyleMask::Borderless | keep);
        w.setHasShadow(false);
    });
}

/// Shows the window without activating the app.
pub fn order_front(window: &Window, cx: &App) {
    after_update(window, cx, |w| w.orderFrontRegardless());
}

pub fn order_out(window: &Window, cx: &App) {
    after_update(window, cx, |w| w.orderOut(None));
}

/// Mirrors the system Reduce Motion setting into GPUI, which then settles
/// transitions and `with_animation` at their end state, and follows changes.
pub fn track_reduce_motion(cx: &mut App) {
    let workspace = NSWorkspace::sharedWorkspace();
    cx.set_reduce_motion(workspace.accessibilityDisplayShouldReduceMotion());
    let app = cx.to_async();
    let block = RcBlock::new(move |_: NonNull<NSNotification>| {
        let reduce = NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceMotion();
        app.update(|cx| cx.set_reduce_motion(reduce));
    });
    // SAFETY: the block runs on the main queue, where GPUI's app lives.
    let observer = unsafe {
        workspace
            .notificationCenter()
            .addObserverForName_object_queue_usingBlock(
                Some(NSWorkspaceAccessibilityDisplayOptionsDidChangeNotification),
                None,
                Some(&NSOperationQueue::mainQueue()),
                &block,
            )
    };
    // The observer lives as long as the app.
    std::mem::forget(observer);
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
pub fn set_origin(window: &Window, cx: &App, origin: Point<Pixels>, height: Pixels) {
    after_update(window, cx, move |w| {
        let flipped_y = main_screen_height() - f64::from(origin.y) - f64::from(height);
        w.setFrameOrigin(NSPoint::new(f64::from(origin.x), flipped_y));
    });
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
