//! Per-event wheel routing for nested settings viewports.

use gpui_kit::*;

/// Clamp each event before the next one arrives. Keeping overscroll until layout
/// makes a quick reversal repay discarded distance instead of moving content.
pub(super) fn consume_delta(offset: Pixels, max: Pixels, delta: Pixels) -> (Pixels, Pixels) {
    let offset = offset.clamp(-max, px(0.));
    let next = (offset + delta).clamp(-max, px(0.));
    (next, delta - (next - offset))
}

/// An invisible event region that respects clipping and modal occlusion.
/// A first-child bubble listener runs after nested content but before the
/// containing Div's built-in scroll listener. Capture can bypass a List's
/// frame-relative accumulated delta when routing a nested viewport ourselves.
pub(super) fn wheel_region(
    phase: DispatchPhase,
    viewport: Option<ScrollHandle>,
    listener: impl Fn(&ScrollWheelEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    canvas(
        move |bounds, window, _| {
            // A canvas inside a scroller moves with its content. Hit-test the
            // tracked viewport instead, including after scrolling a full page.
            let bounds = viewport.as_ref().map_or(bounds, |scroll| scroll.bounds());
            window.insert_hitbox(bounds, HitboxBehavior::Normal)
        },
        move |_, hitbox, window, _| {
            window.on_mouse_event(move |event: &ScrollWheelEvent, event_phase, window, cx| {
                if event_phase == phase && hitbox.should_handle_scroll(window) {
                    listener(event, window, cx);
                }
            });
        },
    )
    .absolute()
    .inset_0()
    .size_full()
}

#[cfg(test)]
mod tests {
    use super::consume_delta;
    use gpui_kit::px;

    #[test]
    fn reversal_moves_immediately_after_overscroll_without_a_frame() {
        let (top, unused) = consume_delta(px(-5.), px(100.), px(105.));
        assert_eq!((top, unused), (px(0.), px(100.)));
        assert_eq!(consume_delta(top, px(100.), px(-0.25)), (px(-0.25), px(0.)));

        let (bottom, unused) = consume_delta(px(-95.), px(100.), px(-105.));
        assert_eq!((bottom, unused), (px(-100.), px(-100.)));
        assert_eq!(
            consume_delta(bottom, px(100.), px(0.25)),
            (px(-99.75), px(0.))
        );
    }

    #[test]
    fn nested_edges_pass_only_unused_distance_and_reversal_stays_inside() {
        let mut inner = px(-90.);
        let mut outer = px(-200.);
        for delta in [-30., 0.125, 0.375, -0.25] {
            let unused;
            (inner, unused) = consume_delta(inner, px(100.), px(delta));
            (outer, _) = consume_delta(outer, px(500.), unused);
        }
        assert_eq!(inner, px(-99.75));
        assert_eq!(outer, px(-220.));
    }

    #[test]
    fn short_content_passes_wheel_distance_without_losing_precision() {
        assert_eq!(
            consume_delta(px(0.), px(0.), px(-0.125)),
            (px(0.), px(-0.125))
        );
        assert_eq!(
            consume_delta(px(-200.), px(50.), px(1.)),
            (px(-49.), px(0.))
        );
    }
}
