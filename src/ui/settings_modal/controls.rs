//! Shared controls and presentation helpers for Settings sections.

use super::*;

/// What Tap mode does with the saved hotkeys. Keys the native listener
/// watches (Globe, a lone right-side modifier) start on a Double tap; key
/// combinations toggle on a single press. Each carries the hotkey's keycap
/// glyphs, or none when several hotkeys share the sentence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum TapStyle {
    DoubleTap(Vec<String>),
    Toggle(Vec<String>),
    /// Some hotkeys double-tap and others toggle.
    Mixed,
}

impl TapStyle {
    /// Names the hotkey in the card copy when there is exactly one.
    pub(super) fn of(hotkeys: &[Hotkey]) -> Self {
        let keys = match hotkeys {
            [only] => hotkey_glyphs(only),
            _ => Vec::new(),
        };
        let listened = hotkeys
            .iter()
            .filter(|h| h.listener_key().is_some())
            .count();
        if listened == hotkeys.len() {
            Self::DoubleTap(keys)
        } else if listened == 0 {
            Self::Toggle(keys)
        } else {
            Self::Mixed
        }
    }

    pub(super) fn keys(&self) -> &[String] {
        match self {
            Self::DoubleTap(keys) | Self::Toggle(keys) => keys,
            Self::Mixed => &[],
        }
    }
}

/// A card sentence whose `{{hotkey}}` is drawn as keycaps, like the Hotkey
/// row above, instead of a run of glyphs squeezed into the prose. Words wrap
/// one at a time so the keycaps flow inline with the text.
pub(super) fn sentence_with_keys(key: &str, keys: &[String], p: &Palette) -> AnyElement {
    if keys.is_empty() {
        return div()
            .child(t_with(
                key,
                &[("hotkey", &t("settings.general.hotkey.theHotkey"))],
            ))
            .into_any_element();
    }
    const SLOT: &str = "\u{1}";
    let text = t_with(key, &[("hotkey", SLOT)]);
    let mut row = div().flex().flex_wrap().items_center().gap_x(px(3.5));
    for (index, part) in text.split(SLOT).enumerate() {
        if index > 0 {
            row = row.child(div().flex().gap(px(2.)).children(keys.iter().map(|k| {
                kbd(k.clone(), p)
                    .h(px(18.))
                    .min_w(px(18.))
                    .text_color(p.muted_foreground)
            })));
        }
        for word in part.split_whitespace() {
            row = row.child(div().child(word.to_string()));
        }
    }
    row.into_any_element()
}

/// Two side-by-side cards, one per activation mode, each explaining itself
/// in a sentence and marked by a radio like the model list. The
/// Tap card describes what the saved hotkey really does: a key combination
/// toggles on one press rather than waiting for a Double tap.
pub(super) fn activation_mode_cards(
    mode: ActivationMode,
    tap: TapStyle,
    push_disabled: Option<String>,
    cx: &mut App,
) -> impl IntoElement + use<> {
    let p = cx.palette();
    let push_blocked = push_disabled.is_some();
    let keys = tap.keys();
    let (tap_title, tap_description) = match &tap {
        TapStyle::DoubleTap(_) => (
            t("common.tap"),
            sentence_with_keys(
                "settings.general.hotkey.activationModeDoubleTapDescription",
                keys,
                &p,
            ),
        ),
        TapStyle::Toggle(_) => (
            t("settings.general.hotkey.toggle"),
            sentence_with_keys(
                "settings.general.hotkey.activationModeToggleDescription",
                keys,
                &p,
            ),
        ),
        TapStyle::Mixed => (
            t("common.tap"),
            div()
                .child(t("settings.general.hotkey.activationModeTapDescription"))
                .into_any_element(),
        ),
    };
    let hold_description = match push_disabled {
        Some(reason) => div().child(reason).into_any_element(),
        None => sentence_with_keys(
            "settings.general.hotkey.activationModeHoldDescription",
            keys,
            &p,
        ),
    };
    let options = [
        (ActivationMode::Tap, "tap", tap_title, tap_description),
        (
            ActivationMode::Push,
            "hold",
            t("common.hold"),
            hold_description,
        ),
    ];
    let mut row = div().flex().flex_wrap().gap(px(8.));
    for (value, key, title, description) in options {
        let selected = mode == value;
        let disabled = value == ActivationMode::Push && push_blocked;
        let id = SharedString::from(format!("settings-activation-{key}"));
        row = row.child(
            div()
                .id(id.clone())
                .when(!disabled, |d| {
                    d.keyboard_focusable(id, Role::RadioButton, title.clone(), cx)
                        .aria_toggled(if selected {
                            Toggled::True
                        } else {
                            Toggled::False
                        })
                        .on_key_down(move |event, window, cx| {
                            move_activation_mode(event, push_blocked, window, cx)
                        })
                })
                .flex_1()
                .min_w(px(200.))
                .flex()
                .flex_col()
                .gap(px(6.))
                .px(px(16.))
                .py(px(14.))
                .rounded(px(RADIUS_XL))
                .border_1()
                .map(|d| {
                    // Selection is a quiet surface and radio dot; the focus ring
                    // remains reserved for keyboard focus.
                    if selected {
                        d.border_color(p.control_border).bg(p.selection)
                    } else {
                        d.border_color(p.border).bg(p.card)
                    }
                })
                .when(disabled, |d| d.opacity(0.5))
                .when(!disabled && !selected, |d| {
                    d.cursor_pointer()
                        .hover(|s| s.bg(p.hover_wash(0.03)))
                        .on_click(move |_, _, cx| {
                            update_settings(cx, |s| s.activation_mode = value)
                        })
                })
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .child(radio_dot(selected, &p))
                        .child(
                            div()
                                .flex_1()
                                .text_size(px(TEXT_BODY))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(p.foreground)
                                .child(title),
                        ),
                )
                .child(
                    div()
                        .pl(px(22.))
                        .text_size(px(TEXT_SMALL))
                        .line_height(px(LINE_SMALL))
                        .text_color(p.subtle_foreground)
                        .child(description),
                ),
        );
    }
    row
}

/// Arrow keys move between the activation cards like a radio group:
/// selecting the neighbour and taking focus to it.
pub(super) fn move_activation_mode(
    event: &KeyDownEvent,
    push_blocked: bool,
    window: &mut Window,
    cx: &mut App,
) {
    let (target, key) = match event.keystroke.key.as_str() {
        "left" | "up" => (ActivationMode::Tap, "tap"),
        "right" | "down" if !push_blocked => (ActivationMode::Push, "hold"),
        _ => return,
    };
    cx.stop_propagation();
    update_settings(cx, |s| s.activation_mode = target);
    let handle = keyed_focus_handle(
        SharedString::from(format!("settings-activation-{key}")).into(),
        cx,
    );
    window.focus(&handle, cx);
}

/// How many listed speech models to offer as choices; a server that lists
/// more still accepts any name typed into the field.
pub(super) const STT_MODEL_CHOICES: usize = 12;

/// The 12 px radio used by the activation cards and the model list; a
/// chosen one takes the accent like every other selection.
pub(super) fn radio_dot(selected: bool, p: &Palette) -> Div {
    div()
        .size(px(12.))
        .flex_none()
        .rounded_full()
        .when(selected, |d| d.border_4().border_color(p.primary))
        .when(!selected, |d| {
            d.border_1().border_color(p.subtle_foreground)
        })
}

/// A 24 px icon button set inside a field, like the API key's save and cancel.
pub(super) fn icon_action(
    id: &'static str,
    label: String,
    name: &'static str,
    color: Hsla,
    cx: &mut App,
) -> Stateful<Div> {
    let wash = cx.palette().hover_wash(0.06);
    div()
        .id(id)
        .keyboard_focusable(id, Role::Button, label, cx)
        .size(px(24.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(RADIUS_SM))
        .cursor_pointer()
        .hover(move |s| s.bg(wash))
        .child(icon(name, 14., color))
}

/// A text action set in a status line, such as Details: a Ghost button's
/// 28 pt target and hover wash, so it reads as a control and not a label.
pub(super) fn ghost_action(
    id: &'static str,
    role: Role,
    label: String,
    cx: &mut App,
) -> Stateful<Div> {
    let p = cx.palette();
    let wash = p.hover_wash(if p.dark { 0.06 } else { 0.05 });
    div()
        .id(id)
        .keyboard_focusable(id, role, label.clone(), cx)
        .min_h(px(28.))
        .px(px(8.))
        .flex()
        .items_center()
        .gap(px(4.))
        .rounded(px(RADIUS_LG))
        .text_size(px(TEXT_SMALL))
        .font_weight(FontWeight::MEDIUM)
        .text_color(p.foreground)
        .cursor_pointer()
        .hover(move |s| s.bg(wash))
        .child(label)
}

/// The status under a server page's Connection label. A failure leads with
/// the cause and next step; the server's own text waits behind Details.
pub(super) fn connection_status(
    check: &ConnectionCheck,
    base: &str,
    details_id: &'static str,
    details_open: bool,
    on_details: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    cx: &mut App,
) -> AnyElement {
    let p = cx.palette();
    let small = |text: String, color: Hsla| {
        div()
            .text_size(px(TEXT_SMALL))
            .line_height(px(LINE_SMALL))
            .text_color(color)
            .child(text)
            .into_any_element()
    };
    match check {
        ConnectionCheck::Idle => small(t("settings.connection.idle"), p.subtle_foreground),
        ConnectionCheck::Checking => small(t("settings.connection.checking"), p.muted_foreground),
        ConnectionCheck::Connected { model_listed } => match model_listed {
            Some(false) => div()
                .flex()
                .items_center()
                .gap(px(6.))
                .child(icon("alert-triangle", 12., p.warning))
                .child(small(t("settings.connection.modelMissing"), p.foreground))
                .into_any_element(),
            Some(true) => {
                status_label(t("settings.connection.connectedWithModel"), &p).into_any_element()
            }
            None => status_label(t("settings.connection.connected"), &p).into_any_element(),
        },
        ConnectionCheck::Failed(raw) => {
            let (headline, details) = friendly_server_error(raw, base);
            let chevron = icon("chevron-down", 12., p.muted_foreground).with_transformation(
                Transformation::rotate(radians(if details_open {
                    0.
                } else {
                    -std::f32::consts::FRAC_PI_2
                })),
            );
            div()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(error_line(headline, &p))
                .when_some(details, |d, details| {
                    // The negative inset lines the label up with the message
                    // while the hover wash keeps its padding.
                    d.child(
                        div().ml(px(12.)).flex().child(
                            ghost_action(
                                details_id,
                                Role::DisclosureTriangle,
                                t("settings.connection.details"),
                                cx,
                            )
                            .aria_expanded(details_open)
                            .child(chevron)
                            .on_click(on_details),
                        ),
                    )
                    .when(details_open, |d| {
                        d.child(
                            div()
                                .ml(px(20.))
                                .mt(px(2.))
                                .px(px(10.))
                                .py(px(8.))
                                .rounded(px(RADIUS_MD))
                                .bg(p.hover_wash(0.04))
                                .font_family(FONT_MONO)
                                .text_size(px(TEXT_MONO_SMALL))
                                .line_height(px(LINE_SMALL))
                                .text_color(p.muted_foreground)
                                .child(details),
                        )
                    })
                })
                .into_any_element()
        }
    }
}

/// The Connection row both server pages share: the status on the left,
/// Test Connection on the right.
pub(super) fn connection_row(
    button_id: &'static str,
    status: AnyElement,
    enabled: bool,
    on_test: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    cx: &mut App,
) -> AnyElement {
    let p = cx.palette();
    panel_row()
        .child(
            div()
                .flex()
                .items_start()
                .justify_between()
                .gap(px(16.))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(
                            div()
                                .text_size(px(TEXT_BODY))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(p.foreground)
                                .child(t("settings.connection.title")),
                        )
                        .child(div().mt(px(2.)).child(status)),
                )
                .child(
                    Button::new(button_id)
                        .label(t("settings.connection.test"))
                        .variant(ButtonVariant::Outline)
                        .size(ButtonSize::Sm)
                        .disabled(!enabled)
                        .on_click(on_test),
                ),
        )
        .into_any_element()
}

/// A small outline chip that fills a field with a value. Chips are
/// shortcuts, not a selection, so none may match; the one that does carries
/// a check. `mono` sets code-like values in the mono step.
pub(super) fn suggestion_chip(
    id: impl Into<ElementId>,
    text: impl Into<SharedString>,
    label: impl Into<SharedString>,
    matches: bool,
    mono: bool,
    enabled: bool,
    cx: &mut App,
) -> Stateful<Div> {
    let p = cx.palette();
    let id: ElementId = id.into();
    let (fg, hover_fg) = (p.foreground, p.foreground);
    let hover_border = p.border_hover;
    let hover_wash = p.hover_wash(0.04);
    let chip = div().id(id.clone());
    let chip = if enabled {
        chip.keyboard_focusable(id, Role::Button, label, cx)
            .cursor_pointer()
            .hover(move |s| {
                s.border_color(hover_border)
                    .text_color(hover_fg)
                    .bg(hover_wash)
            })
    } else {
        chip
    };
    chip.h(px(24.))
        .px(px(8.))
        .flex()
        .items_center()
        .gap(px(4.))
        .rounded(px(RADIUS_MD))
        .border_1()
        .border_color(if matches {
            p.foreground.opacity(if p.dark { 0.35 } else { 0.3 })
        } else {
            p.border
        })
        .when(matches, |d| d.bg(p.hover_wash(0.04)))
        .text_size(px(if mono { TEXT_MONO_SMALL } else { TEXT_SMALL }))
        .when(mono, |d| d.font_family(FONT_MONO))
        .font_weight(FontWeight::MEDIUM)
        .text_color(if matches { fg } else { p.muted_foreground })
        .whitespace_nowrap()
        .when(matches, |d| d.child(icon("check", 12., fg)))
        .child(text.into())
}

/// A settings row whose whole surface flips its switch, so the hit target
/// is the row and not only the 30×18 track.
pub(super) fn toggle_row(
    id: &'static str,
    label: String,
    description: Option<String>,
    checked: bool,
    disabled: bool,
    on_change: impl Fn(bool, &mut Window, &mut App) + 'static,
    p: &Palette,
) -> AnyElement {
    let on_change: Rc<dyn Fn(bool, &mut Window, &mut App)> = Rc::new(on_change);
    let on_switch = on_change.clone();
    let toggle = Toggle::new(id, checked)
        .label(label.clone())
        .disabled(disabled)
        .on_change(move |value, window, cx| {
            // The row's own click would flip it straight back.
            cx.stop_propagation();
            on_switch(value, window, cx)
        });
    let wash = p.hover_wash(0.025);
    settings_row(label, description.map(Into::into), toggle, p)
        .id((id, 1usize))
        // Rounded so the wash stays inside the card's corners on the first
        // and last rows.
        .rounded(px(RADIUS_XL - 1.))
        .when(!disabled, |d| {
            d.cursor_pointer()
                .hover(move |s| s.bg(wash))
                .on_click(move |_, window, cx| on_change(!checked, window, cx))
        })
        .into_any_element()
}

/// An error sentence led by an alert icon; the icon carries the color so
/// the text stays readable.
pub(super) fn error_line(message: String, p: &Palette) -> AnyElement {
    div()
        .flex()
        .items_start()
        .gap(px(6.))
        .child(div().h(px(LINE_SMALL)).flex().items_center().child(icon(
            "alert-circle",
            14.,
            p.destructive,
        )))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_size(px(TEXT_SMALL))
                .line_height(px(LINE_SMALL))
                .text_color(p.foreground)
                .child(message),
        )
        .into_any_element()
}

/// Turns a server error into a sentence with the next step, plus the raw
/// text when it adds detail. Messages that are already localized pass
/// through as they are.
pub(super) fn friendly_server_error(raw: &str, base: &str) -> (String, Option<String>) {
    let raw = raw.trim();
    let host = endpoint_host(base);
    let lower = raw.to_ascii_lowercase();
    let status = raw
        .split_whitespace()
        .next()
        .filter(|code| code.len() == 3 && code.chars().all(|c| c.is_ascii_digit()));
    let headline = if lower.starts_with("error sending request")
        || lower.contains("connection refused")
        || lower.contains("dns error")
        || lower.contains("connect")
    {
        t_with("settings.serverErrors.unreachable", &[("host", &host)])
    } else if lower.contains("timed out") {
        t_with("settings.serverErrors.timedOut", &[("host", &host)])
    } else if let Some(status) = status {
        t_with("settings.serverErrors.status", &[("status", status)])
    } else {
        return (raw.to_string(), None);
    };
    (headline, Some(raw.to_string()))
}

pub(super) fn info_line(label: String, value: String, p: &Palette) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(8.))
        .child(
            div()
                .text_size(px(TEXT_SMALL))
                .text_color(p.subtle_foreground)
                .child(label),
        )
        .child(
            div()
                .font_family(FONT_MONO)
                .text_size(px(TEXT_MONO_SMALL))
                .font_weight(FontWeight::MEDIUM)
                .text_color(p.foreground)
                .child(value),
        )
}

/// Keep grouped aliases searchable and preserve the saved API name.
pub(super) fn cleanup_model_options(models: &[ModelOption], selected: &str) -> Vec<SelectOption> {
    models
        .iter()
        .map(|model| {
            let value = model.selected_value(selected).to_string();
            SelectOption::new(value.clone(), value).keywords(format!(
                "{} {}",
                model.value,
                model.aliases.join(" ")
            ))
        })
        .collect()
}

pub(super) fn permission_row(
    id: &'static str,
    title: String,
    description: String,
    granted: bool,
    on_request: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    p: &Palette,
) -> AnyElement {
    let control = if granted {
        status_label(t("settings.permissions.granted"), p).into_any_element()
    } else {
        Button::new(id)
            .label(t("settings.permissions.grantAccess"))
            .size(ButtonSize::Sm)
            .on_click(on_request)
            .into_any_element()
    };
    settings_row(title, Some(description.into()), control, p).into_any_element()
}
