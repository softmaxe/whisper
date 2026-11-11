//! Error and recovery cards, their actions, and toast rendering.

use super::*;

/// Inner padding of the error and recovery panels.
const PANEL_PAD: f32 = 16.;
/// The panels' leading icon column; body, content and actions start after it.
const PANEL_ICON: f32 = 16.;
const PANEL_ICON_GAP: f32 = 10.;
/// The dismiss button of a panel, sized like the pill's cancel control. A
/// faint track outlines it as a button, and the countdown drains over that
/// track in a hairline, so it never reads as a loading spinner.
const DISMISS_SIZE: f32 = 28.;
const DISMISS_STROKE: f32 = 1.5;
const DISMISS_TRACK: f32 = 0.14;
const DISMISS_DRAIN: f32 = 0.4;
/// How far the dismiss button reaches past the title line on each side. It
/// pulls up and right by as much, so it sits the same distance from the
/// card's top and right edges.
const DISMISS_OUTSET: f32 = (DISMISS_SIZE - LINE_SMALL) / 2.;
/// The right inset of the panel's text, clearing the dismiss button.
const DISMISS_INSET: f32 = DISMISS_SIZE - DISMISS_OUTSET + 8.;

impl PillView {
    /// The close button shared by both panels. A filled disc with a faint
    /// track reads as a button; the panel's countdown drains brighter over
    /// that track from twelve o'clock, so it never looks like an
    /// indeterminate spinner. When Escape also closes the panel, the tooltip
    /// says so rather than a second, unclickable key chip beside it.
    /// VoiceOver hears the time left, which the ring only shows.
    fn dismiss_button(
        id: &'static str,
        countdown: Option<&Countdown>,
        escape: bool,
        on_close: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
        cx: &mut App,
    ) -> Stateful<Div> {
        let p = hud(cx);
        let now = Instant::now();
        let remaining = countdown.map_or(0., |c| c.remaining(now));
        let closes_in = countdown.map(|c| {
            crate::i18n::t_with(
                "dictation.pill.closesIn",
                &[("seconds", &c.seconds_left(now).to_string())],
            )
        });
        let ring = canvas(
            |_, _, _| {},
            move |bounds, _, window, _| {
                let center = bounds.center();
                let r =
                    f32::from(bounds.size.width.min(bounds.size.height)) / 2. - DISMISS_STROKE / 2.;
                let arc = |share: f32, color: Hsla, window: &mut Window| {
                    let steps = ((64. * share).ceil() as usize).max(2);
                    let mut path = PathBuilder::stroke(px(DISMISS_STROKE));
                    for i in 0..=steps {
                        // From twelve o'clock, clockwise.
                        let a = -FRAC_PI_2 + TAU * share * i as f32 / steps as f32;
                        let at = point(center.x + px(r * a.cos()), center.y + px(r * a.sin()));
                        if i == 0 {
                            path.move_to(at);
                        } else {
                            path.line_to(at);
                        }
                    }
                    if let Ok(path) = path.build() {
                        window.paint_path(path, color);
                    }
                };
                arc(1., p.white.opacity(DISMISS_TRACK), window);
                if remaining > 0.005 {
                    arc(remaining, p.white.opacity(DISMISS_DRAIN), window);
                }
            },
        )
        .absolute()
        .inset_0();
        let label: SharedString = t("dictation.pill.dismiss").into();
        div()
            .id(id)
            .keyboard_focusable(id, Role::Button, label.clone(), cx)
            .when_some(closes_in, |d, closes_in| d.aria_description(closes_in))
            .when(escape, |d| d.aria_keyshortcuts("Escape"))
            .relative()
            .flex_none()
            .size(px(DISMISS_SIZE))
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .bg(p.white.opacity(0.06))
            .cursor_pointer()
            .hover(move |s| s.bg(p.white.opacity(0.12)))
            .active(move |s| s.bg(p.white.opacity(0.18)))
            .tooltip(move |_, cx| {
                cx.new(|_| HudTip {
                    label: label.clone(),
                    key: escape.then_some("Esc"),
                })
                .into()
            })
            .child(ring)
            .child(icon("x", 13., p.foreground))
            .on_click(on_close)
    }

    /// A compact panel action: white on the HUD when primary, a quiet wash
    /// otherwise. The primary dims on hover, since a lighter white than its
    /// near-white rest would not show; every action dims further while
    /// pressed, so a click lands before the card closes.
    fn panel_action(
        id: &'static str,
        icon_name: &'static str,
        label: String,
        primary: bool,
        cx: &mut App,
    ) -> Stateful<Div> {
        let p = hud(cx);
        // The primary action takes the app's accent, as primary actions do
        // in the main window.
        let (bg, hover, fg) = if primary {
            (p.primary, p.primary.opacity(0.88), p.primary_foreground)
        } else {
            (p.white.opacity(0.1), p.white.opacity(0.16), p.foreground)
        };
        div()
            .id(id)
            .keyboard_focusable(id, Role::Button, label.clone(), cx)
            .h(px(28.))
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .gap(px(6.))
            .px(px(10.))
            .rounded(px(RADIUS_LG))
            .bg(bg)
            .hover(move |s| s.bg(hover))
            .active(|s| s.opacity(0.8))
            .text_size(px(TEXT_SMALL))
            .font_weight(FontWeight::MEDIUM)
            .text_color(fg)
            .whitespace_nowrap()
            .cursor_pointer()
            .child(icon(icon_name, 14., fg))
            .child(label)
    }

    /// The HUD card both panels sit on.
    fn panel_card(id: &'static str, cx: &App) -> Stateful<Div> {
        let p = hud(cx);
        div()
            .id(id)
            .relative()
            .w_full()
            .flex()
            .flex_col()
            .p(px(PANEL_PAD))
            .rounded(px(RADIUS_XL))
            .border_1()
            .border_color(hud_edge())
            .bg(p.toast_surface)
            .shadow(hud_shadow(12., 32., -12., 0.35))
    }

    /// The header both panels share: a status icon, the title and the close
    /// button on one line, then the body under the title. Everything after
    /// the header sits in [`Self::panel_column`], so the panels share one
    /// grid.
    fn panel_header(
        icon_name: &'static str,
        accent: Hsla,
        title: String,
        body: Option<String>,
        dismiss: Stateful<Div>,
        cx: &App,
    ) -> Div {
        let p = hud(cx);
        let line = div()
            .flex()
            .items_start()
            .gap(px(PANEL_ICON_GAP))
            .child(
                div()
                    .h(px(LINE_SMALL))
                    .flex()
                    .items_center()
                    .child(icon(icon_name, PANEL_ICON, accent)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .pr(px(DISMISS_INSET))
                    .text_size(px(TEXT_BODY))
                    .line_height(px(LINE_SMALL))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(p.foreground)
                    .child(title),
            );
        // The close button floats centered on the title line, so its size
        // does not push the body down. It reaches into the body's first
        // line, so the body keeps the title's right inset and never runs
        // under it.
        div()
            .relative()
            .flex_none()
            .child(line)
            .child(
                div()
                    .absolute()
                    .top(px(-DISMISS_OUTSET))
                    .right(px(-DISMISS_OUTSET))
                    .child(dismiss),
            )
            .when_some(body.filter(|b| !b.is_empty()), |d, body| {
                d.child(
                    Self::panel_column()
                        .mt(px(2.))
                        .pr(px(DISMISS_INSET))
                        .text_size(px(TEXT_SMALL))
                        .line_height(px(LINE_SMALL))
                        .text_color(p.muted_foreground)
                        .child(body),
                )
            })
    }

    /// The text column, inset past the header icon.
    fn panel_column() -> Div {
        div().pl(px(PANEL_ICON + PANEL_ICON_GAP))
    }

    pub(super) fn render_error(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let p = hud(cx);
        let reduce = cx.reduce_motion();
        let error = self.error.as_ref()?;
        let countdown = error.countdown;
        let title = error.title.clone();
        let transcript = error.transcript.clone();
        let actions = error.actions;
        let kept = error.kept;
        // The kept recording's length belongs in the sentence, not on the
        // button, where an m:ss figure next to the countdown ring reads as
        // time left.
        let description = match kept {
            Some(length) if actions.retry => format!(
                "{} {}",
                error.description,
                crate::i18n::t_with(
                    "dictation.keptRecording",
                    &[("time", &format_elapsed(length))]
                )
            ),
            _ => error.description.to_string(),
        };

        // When the recording was kept, sending it again is the fix that
        // costs the user nothing, so it leads, named so it cannot be
        // mistaken for Record New.
        let retry = actions.retry.then(|| {
            let label = t("dictation.errorActions.resend");
            Self::panel_action("pill-error-retry", "rotate-ccw", label, true, cx).on_click(
                cx.listener(|this, _, _, cx| {
                    this.error = None;
                    Dictation::global(cx).update(cx, |d, cx| d.retry_failed(cx));
                    cx.notify();
                }),
            )
        });
        // Recording again leads only when nothing else can recover the
        // words: not when they were kept or survived as text.
        let record = Self::panel_action(
            "pill-error-record",
            "mic",
            t("dictation.errorActions.recordNew"),
            transcript.is_none() && !actions.retry,
            cx,
        )
        .on_click(cx.listener(|this, _, _, cx| {
            this.error = None;
            Dictation::global(cx).update(cx, |d, cx| d.start(cx));
        }));
        // The server is set in Settings → Speech to text. The card stays,
        // and its countdown holds while the sheet is open and starts over
        // once it closes, so the words can be resent after the fix.
        let settings = actions.open_settings.then(|| {
            Self::panel_action(
                "pill-error-settings",
                "settings",
                t("dictation.errorActions.openSettings"),
                false,
                cx,
            )
            .on_click(cx.listener(|this, _, _, cx| {
                if let Some(error) = this.error.as_mut() {
                    error.restart();
                }
                this.set_hovered(this.hovered);
                cx.defer(|cx| {
                    crate::app::show_settings(
                        Some(super::super::settings_modal::SettingsSection::SpeechToText),
                        cx,
                    )
                });
                cx.notify();
            }))
        });
        let view = transcript.map(|(transcript, copied)| {
            let recovery_title = title.clone();
            Self::panel_action(
                "pill-error-transcript",
                "scroll-text",
                t("dictation.errorActions.viewTranscript"),
                true,
                cx,
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.error = None;
                this.recovery = Some(Recovery::new(
                    recovery_title.clone(),
                    None,
                    transcript.clone(),
                    copied,
                ));
                this.set_hovered(this.hovered);
                cx.notify();
            }))
        });
        let dismiss = Self::dismiss_button(
            "pill-error-close",
            countdown.as_ref(),
            false,
            cx.listener(|this, _, _, cx| {
                this.close_error(cx);
                cx.notify();
            }),
            cx,
        );
        // Actions start in the text column, primary first, so the panel
        // reads top to bottom without a stray button on the far side.
        let actions = Self::panel_column()
            .mt(px(12.))
            .flex()
            .flex_wrap()
            .gap(px(8.))
            .children(retry)
            .children(view)
            .child(record)
            .children(settings);
        Some(
            Self::panel_card("pill-error", cx)
                .child(Self::panel_header(
                    "alert-circle",
                    p.destructive,
                    title,
                    Some(description),
                    dismiss,
                    cx,
                ))
                .child(actions)
                .with_animation(
                    "pill-error-in",
                    motion(DUR_BASE, cx).with_easing(ease_out),
                    move |d, t| {
                        let d = d.opacity(t);
                        if reduce { d } else { d.mt(px(4. * (1. - t))) }
                    },
                )
                .into_any_element(),
        )
    }

    pub(super) fn render_recovery(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let p = hud(cx);
        let reduce = cx.reduce_motion();
        let recovery = self.recovery.as_ref()?;
        let countdown = recovery.countdown;
        let ready = !recovery.copy_failed && recovery.copied;
        let failed = recovery.copy_failed;
        let title = recovery.title.clone();
        let description = recovery.description.clone();
        let text = recovery.text.clone();

        let dismiss = Self::dismiss_button(
            "pill-recovery-close",
            recovery.copied.then_some(&countdown),
            true,
            cx.listener(|this, _, _, cx| {
                this.recovery = None;
                cx.notify();
            }),
            cx,
        );
        // The words sit in an inset well so they read as the thing to keep,
        // apart from the panel's own copy.
        let transcript = Self::panel_column()
            .mt(px(12.))
            .flex_1()
            .min_h_0()
            .flex()
            .child(
                div()
                    .id("pill-recovery-text")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px(px(12.))
                    .py(px(10.))
                    .rounded(px(RADIUS_LG))
                    .bg(p.white.opacity(0.05))
                    .font_family(FONT_VOICE)
                    .text_size(px(TEXT_READING))
                    .line_height(px(LINE_READING))
                    .text_color(p.foreground)
                    .child(text.clone()),
            );
        // One line says where the text stands; Copy shows only while the
        // clipboard does not hold it.
        let status = if ready {
            Some((
                "check",
                p.success,
                t("transcriptionPreview.recovery.copiedHint"),
            ))
        } else if failed {
            Some((
                "alert-circle",
                p.destructive,
                t("transcriptionPreview.recovery.copyFailed"),
            ))
        } else {
            None
        };
        let footer = Self::panel_column()
            .mt(px(12.))
            .flex_none()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(12.))
            .min_h(px(28.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .min_w_0()
                    .text_size(px(TEXT_SMALL))
                    .line_height(px(LINE_SMALL))
                    .text_color(p.muted_foreground)
                    .when_some(status, |d, (name, color, label)| {
                        d.child(icon(name, 14., color)).child(label)
                    }),
            )
            .when(!ready, |d| {
                d.child(
                    Self::panel_action(
                        "pill-recovery-copy",
                        "copy",
                        t("transcriptionPreview.recovery.copy"),
                        true,
                        cx,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let ok = backend::write_clipboard(&text);
                        if let Some(r) = this.recovery.as_mut() {
                            r.copied = ok;
                            r.copy_failed = !ok;
                            if ok {
                                // Copied: the panel only needs to stay long
                                // enough to read the paste hint.
                                r.countdown = Countdown::new(RECOVERY_COPIED, Instant::now());
                            }
                        }
                        this.set_hovered(this.hovered);
                        cx.notify();
                    })),
                )
            });
        Some(
            Self::panel_card("pill-recovery", cx)
                .max_h(px(420.))
                .overflow_hidden()
                .child(Self::panel_header(
                    "alert-triangle",
                    p.warning,
                    title,
                    description,
                    dismiss,
                    cx,
                ))
                .child(transcript)
                .child(footer)
                .with_animation(
                    "pill-recovery-in",
                    motion(DUR_SLOW, cx).with_easing(ease_out),
                    move |d, t| {
                        let d = d.opacity((t * 2.).min(1.));
                        if reduce { d } else { d.mt(px(6. * (1. - t))) }
                    },
                )
                .into_any_element(),
        )
    }

    pub(super) fn render_toasts(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.toasts.is_empty() {
            return None;
        }
        let p = hud(cx);
        let mut stack = div().flex().flex_col().gap(px(6.)).w(px(TOAST_W));
        for (i, toast) in self.toasts.iter().enumerate() {
            let (icon_name, accent) = match toast.kind {
                ToastKind::Destructive => ("alert-circle", p.destructive),
                ToastKind::Success => ("check", p.success),
                ToastKind::Hint => ("keyboard", p.muted_foreground),
            };
            let undo = toast.undo_words.clone();
            stack = stack.child(
                div()
                    .id(("pill-toast", i))
                    .flex()
                    .items_start()
                    .gap(px(10.))
                    .px(px(12.))
                    .py(px(10.))
                    .rounded(px(RADIUS_LG))
                    .border_1()
                    .border_color(hud_edge())
                    .bg(p.toast_surface)
                    .shadow(hud_shadow(8., 20., -6., 0.3))
                    .child(
                        div()
                            .h(px(LINE_SMALL))
                            .flex()
                            .items_center()
                            .child(icon(icon_name, 14., accent)),
                    )
                    .child(
                        div()
                            .flex_1()
                            // Let long descriptions wrap inside the card.
                            .min_w_0()
                            .text_size(px(TEXT_SMALL))
                            .line_height(px(LINE_SMALL))
                            .child(
                                div()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(p.foreground)
                                    .child(toast.title.clone()),
                            )
                            .when_some(toast.description.clone(), |d, desc| {
                                d.child(div().mt(px(2.)).text_color(p.muted_foreground).child(desc))
                            }),
                    )
                    .when_some(undo, |d, words| {
                        let id = ElementId::from(("pill-toast-undo", i));
                        d.child(
                            div()
                                .id(id.clone())
                                .keyboard_focusable(id, Role::Button, t("app.toasts.undo"), cx)
                                .flex_none()
                                .self_center()
                                .h(px(24.))
                                .flex()
                                .items_center()
                                .px(px(8.))
                                .rounded(px(RADIUS_MD))
                                .bg(p.white.opacity(0.1))
                                .text_size(px(TEXT_SMALL))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(p.foreground)
                                .cursor_pointer()
                                .hover(move |s| s.bg(p.white.opacity(0.16)))
                                .child(t("app.toasts.undo"))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    backend::undo_learned_words(&words, cx);
                                    this.toasts
                                        .retain(|t| t.undo_words.as_ref() != Some(&words));
                                    cx.notify();
                                })),
                        )
                    }),
            );
        }
        Some(stack.into_any_element())
    }
}
