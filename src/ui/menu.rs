//! Shared popup surfaces, positioning, keyboard navigation and focus return.

use std::rc::Rc;

use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use super::components::{DismissMenu, MENU_CONTEXT, MenuTriggerBounds, keyed_focus_handle};
use super::overlays::Overlays;
use super::theme::{DUR_SNAP, PaletteExt, RADIUS_LG, ease_snap, motion};

/// Clear popup state even when an overlay replaces its rendered focus tree.
pub fn dismiss_on_overlay<V: 'static>(cx: &mut Context<V>, dismiss: impl Fn(&mut V) + 'static) {
    cx.subscribe(
        &Overlays::global(cx),
        move |this, _, _: &DismissMenu, cx| {
            dismiss(this);
            cx.notify();
        },
    )
    .detach();
}

#[derive(Clone)]
pub struct MenuFocus {
    items: Vec<FocusHandle>,
    trigger: FocusHandle,
}

impl MenuFocus {
    pub fn new(
        items: impl IntoIterator<Item = ElementId>,
        trigger: ElementId,
        cx: &mut App,
    ) -> Self {
        Self {
            items: items
                .into_iter()
                .map(|key| keyed_focus_handle(key, cx))
                .collect(),
            trigger: keyed_focus_handle(trigger, cx),
        }
    }

    pub fn restore(&self, always: bool, window: &mut Window, cx: &mut App) {
        if always || self.items.iter().any(|handle| handle.is_focused(window)) {
            window.focus(&self.trigger, cx);
        }
    }

    fn step(&self, up: bool, first_if_unfocused: bool, window: &mut Window, cx: &mut App) {
        if let Some(next) = next_item(
            self.items.len(),
            self.items
                .iter()
                .position(|handle| handle.is_focused(window)),
            up,
            first_if_unfocused,
        ) {
            window.focus(&self.items[next], cx);
        }
    }
}

fn next_item(
    count: usize,
    current: Option<usize>,
    up: bool,
    first_if_unfocused: bool,
) -> Option<usize> {
    if count == 0 {
        return None;
    }
    Some(match current {
        Some(index) if up => (index + count - 1) % count,
        Some(index) => (index + 1) % count,
        None if up && !first_if_unfocused => count - 1,
        None => 0,
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum MenuAppearance {
    History,
    Dictionary,
}

pub enum MenuPlacement {
    /// History menus use GPUI's trigger-relative anchor and window snapping.
    TriggerRight,
    /// The Dictionary toolbar keeps its existing absolute right alignment.
    ToolbarRight {
        top: Pixels,
    },
    Pointer(Point<Pixels>),
}

pub struct PopupMenu {
    pub id: ElementId,
    pub min_width: f32,
    pub appearance: MenuAppearance,
    pub placement: MenuPlacement,
    pub trigger: Option<MenuTriggerBounds>,
    pub focus: MenuFocus,
}

struct MenuSession {
    surface: FocusHandle,
    _blur: Subscription,
}

fn dismiss_on_focus_change(menu_focused: bool, trigger_focused: bool) -> bool {
    // Keep trigger press/release toggling intact; an overlay or another control
    // taking focus ends the popup without returning focus to its trigger.
    !menu_focused && !trigger_focused
}

impl PopupMenu {
    pub fn render<V: 'static>(
        self,
        items: impl IntoIterator<Item = AnyElement>,
        on_dismiss: impl Fn(&mut V, &mut Window, &mut Context<V>) + 'static,
        window: &mut Window,
        cx: &mut Context<V>,
    ) -> AnyElement {
        let dismiss = Rc::new(on_dismiss);
        let blur_dismiss = dismiss.clone();
        let blur_focus = self.focus.clone();
        let on_blur = cx.listener(move |this, _: &FocusOutEvent, window, cx| {
            if dismiss_on_focus_change(
                blur_focus.items.iter().any(|item| item.is_focused(window)),
                blur_focus.trigger.is_focused(window),
            ) {
                blur_dismiss(this, window, cx);
            }
        });
        // Initialize once per opening, after the menu enters the focus tree.
        // Pointer and keyboard opens must both route keys through the menu.
        let initial_focus = self.focus.clone();
        let opening_focus = window.focused(cx);
        let session = window.use_keyed_state(self.id.clone(), cx, |window, cx| {
            let surface = cx.focus_handle();
            let blur = window.on_focus_out(&surface, cx, move |event, window, cx| {
                on_blur(&event, window, cx);
            });
            window.on_next_frame(move |window, cx| {
                if window.focused(cx) == opening_focus
                    && let Some(first) = initial_focus.items.first()
                {
                    window.focus(first, cx);
                }
            });
            MenuSession {
                surface,
                _blur: blur,
            }
        });
        let p = cx.palette();
        let dictionary = self.appearance == MenuAppearance::Dictionary;
        let outside = dismiss.clone();
        let keyboard = dismiss.clone();
        let action_focus = self.focus.clone();
        let key_focus = self.focus;
        let trigger = self.trigger;
        let menu = div()
            .id(self.id)
            .track_focus(&session.read(cx).surface)
            .role(Role::Menu)
            .occlude()
            .min_w(px(self.min_width))
            .p(px(4.))
            .flex()
            .flex_col()
            .rounded(px(RADIUS_LG))
            .border_1()
            .border_color(if dictionary {
                p.foreground.opacity(0.1)
            } else if p.dark {
                p.border_hover
            } else {
                p.border.opacity(0.7)
            })
            .bg(if dictionary || !p.dark {
                p.popover
            } else {
                p.surface_3
            })
            .when(dictionary, |d| {
                d.shadow(vec![BoxShadow {
                    color: hsla(0., 0., 0., if p.dark { 0.55 } else { 0.22 }),
                    offset: point(px(0.), px(16.)),
                    blur_radius: px(40.),
                    spread_radius: px(-18.),
                    inset: false,
                }])
            })
            .when(!dictionary, |d| d.shadow_lg())
            .on_mouse_down_out(
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    if !trigger
                        .as_ref()
                        .is_some_and(|bounds| bounds.contains(event.position))
                    {
                        outside(this, window, cx);
                    }
                }),
            )
            .key_context(MENU_CONTEXT)
            .on_action(cx.listener(move |this, _: &DismissMenu, window, cx| {
                action_focus.restore(dictionary, window, cx);
                keyboard(this, window, cx);
            }))
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                match event.keystroke.key.as_str() {
                    "escape" => {
                        key_focus.restore(dictionary, window, cx);
                        dismiss(this, window, cx);
                    }
                    "down" | "up" => {
                        key_focus.step(event.keystroke.key == "up", dictionary, window, cx);
                    }
                    _ => return,
                }
                cx.stop_propagation();
            }))
            .children(items);
        let menu = if dictionary {
            menu.with_animation(
                "dict-menu-in",
                motion(DUR_SNAP, cx).with_easing(ease_snap),
                |d, t| d.opacity(t),
            )
            .into_any_element()
        } else {
            menu.mt(px(4.)).into_any_element()
        };
        match self.placement {
            MenuPlacement::TriggerRight => deferred(
                anchored()
                    .anchor(Anchor::TopRight)
                    .snap_to_window_with_margin(px(8.))
                    .child(menu),
            )
            .with_priority(1)
            .into_any_element(),
            MenuPlacement::ToolbarRight { top } => {
                deferred(div().absolute().top(top).right_0().child(menu))
                    .with_priority(2)
                    .into_any_element()
            }
            MenuPlacement::Pointer(at) => deferred(
                anchored()
                    .position(at)
                    .snap_to_window_with_margin(px(8.))
                    .child(menu),
            )
            .with_priority(2)
            .into_any_element(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{dismiss_on_focus_change, next_item};

    #[test]
    fn leaving_for_an_overlay_dismisses_without_breaking_trigger_toggling() {
        assert!(dismiss_on_focus_change(false, false));
        assert!(!dismiss_on_focus_change(false, true));
        assert!(!dismiss_on_focus_change(true, false));
    }

    #[test]
    fn arrow_navigation_wraps_and_preserves_initial_focus_rules() {
        assert_eq!(next_item(3, Some(2), false, false), Some(0));
        assert_eq!(next_item(3, Some(0), true, false), Some(2));
        assert_eq!(next_item(3, None, true, false), Some(2));
        assert_eq!(next_item(3, None, true, true), Some(0));
        assert_eq!(next_item(0, None, true, false), None);
    }

    #[test]
    fn a_single_item_menu_takes_focus_and_keeps_it_on_either_arrow() {
        assert_eq!(next_item(1, None, false, true), Some(0));
        assert_eq!(next_item(1, Some(0), false, false), Some(0));
        assert_eq!(next_item(1, Some(0), true, false), Some(0));
    }
}
