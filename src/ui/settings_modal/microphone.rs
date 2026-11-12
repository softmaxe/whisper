//! Microphone behavior and presentation for SettingsModal.

use super::*;
use crate::audio::microphone_test::{Problem, State};

impl SettingsModal {
    pub(super) fn reload_microphones(&mut self, cx: &mut Context<Self>) {
        let _ = self.mic_refresh.try_send(());
        // Repeated clicks still request a refresh without interrupting a turn.
        if !cx.reduce_motion()
            && self.mic_refresh_started.is_none_or(|started| {
                started.elapsed() >= Duration::from_millis(MIC_REFRESH_ANIMATION_MS)
            })
        {
            self.mic_refresh_started = Some(Instant::now());
            cx.notify();
        }
    }

    pub(super) fn sync_microphone_select(&mut self, cx: &mut Context<Self>) {
        let s = settings(cx);
        let is_auto = s.microphone_selection_mode == MicrophoneSelectionMode::Auto;
        let selected_device = self.selected_device(&s);
        let unavailable_label = t_with(
            "microphoneSettings.unavailableDevice",
            &[(
                "device",
                &if s.microphone_selection_mode == MicrophoneSelectionMode::BuiltIn {
                    t("microphoneSettings.builtInDevice")
                } else if s.selected_mic_device_label.is_empty() {
                    t("microphoneSettings.unknownDevice")
                } else {
                    s.selected_mic_device_label.clone()
                },
            )],
        );
        let mut options = vec![SelectOption::new(
            "__auto__",
            t("microphoneSettings.auto.label"),
        )];
        if !is_auto && selected_device.is_none() {
            options.push(
                SelectOption::new("__unavailable__", unavailable_label.clone()).disabled(true),
            );
        }
        for device in &self.mic_snapshot.devices {
            options.push(SelectOption::new(device.id.clone(), device.label.clone()));
        }
        let value: SharedString = if is_auto {
            "__auto__".into()
        } else {
            selected_device
                .map(|d| d.id.clone())
                .unwrap_or_else(|| "__unavailable__".into())
                .into()
        };
        self.microphone.update(cx, |sel, cx| {
            sel.set_options(options, cx);
            sel.set_selected(Some(value), cx);
            sel.set_placeholder(t("microphoneSettings.selectPlaceholder"));
        });
    }

    pub(super) fn selected_device(&self, s: &Settings) -> Option<MicDevice> {
        self.mic_snapshot.resolve(s)
    }

    /// The input device row: the select sits inline like every other
    /// select, and what Auto does is a quiet note under the label.
    pub(super) fn render_microphone(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = cx.palette();
        let rotation = match self.mic_refresh_started {
            Some(started)
                if !cx.reduce_motion()
                    && started.elapsed() < Duration::from_millis(MIC_REFRESH_ANIMATION_MS) =>
            {
                window.request_animation_frame();
                -cubic_bezier(
                    0.22,
                    0.7,
                    0.22,
                    1.,
                    progress(started.elapsed(), MIC_REFRESH_ANIMATION_MS),
                )
            }
            _ => {
                self.mic_refresh_started = None;
                0.
            }
        };
        let s = settings(cx);
        let is_auto = s.microphone_selection_mode == MicrophoneSelectionMode::Auto;
        let note = if is_auto {
            t("microphoneSettings.auto.description")
        } else {
            t("microphoneSettings.helpText")
        };
        let busy = AppState::global(cx).read(cx).dictation.is_active();
        panel_row()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(32.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div().flex().items_center().gap(px(4.)).child(
                                    div()
                                        .text_size(px(TEXT_BODY))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(p.foreground)
                                        .child(t("microphoneSettings.inputDevice")),
                                ),
                            )
                            .child(
                                div()
                                    .mt(px(2.))
                                    .max_w(px(crate::ui::components::DESCRIPTION_MAX_WIDTH))
                                    .text_size(px(TEXT_SMALL))
                                    .line_height(px(LINE_SMALL))
                                    .text_color(p.subtle_foreground)
                                    .child(note),
                            ),
                    )
                    // Refresh follows the device picker at the row's trailing edge.
                    .child(
                        div()
                            .flex_none()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .child(
                                div()
                                    .id("settings-test-mic-tooltip")
                                    .when(busy, |d| {
                                        d.tooltip(crate::ui::components::text_tooltip(t(
                                            "microphoneTest.busy",
                                        )))
                                    })
                                    .child(
                                        Button::new("settings-test-mic")
                                            .icon("mic", 14.)
                                            .label(t("microphoneTest.test"))
                                            .variant(ButtonVariant::Outline)
                                            .size(ButtonSize::Sm)
                                            .disabled(busy)
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.start_mic_test(cx)
                                            })),
                                    ),
                            )
                            .child(self.microphone.clone())
                            .child(
                                Button::new("settings-refresh-mics")
                                    .icon("refresh-cw", 14.)
                                    .icon_rotation(rotation)
                                    .aria_label(t("microphoneSettings.refresh"))
                                    .variant(ButtonVariant::Ghost)
                                    .size(ButtonSize::Sm)
                                    .on_click(
                                        cx.listener(|this, _, _, cx| this.reload_microphones(cx)),
                                    ),
                            ),
                    ),
            )
            .into_any_element()
    }

    pub(super) fn cancel_mic_test(&mut self) {
        self.mic_test_request = None;
        self.mic_test.lock().close();
    }

    fn start_mic_test(&mut self, cx: &mut Context<Self>) {
        if self.closing || AppState::global(cx).read(cx).dictation.is_active() {
            return;
        }
        let device = backend::preferred_input_device(&settings(cx));
        let (generation, start) = self
            .mic_test
            .lock()
            .start(device.as_deref(), Instant::now());
        self.mic_test_request = Some(cx.spawn(async move |this, cx| {
            let result = start.await;
            let _ = this.update(cx, |this, cx| {
                this.mic_test
                    .lock()
                    .capture_started(generation, result, Instant::now());
                cx.notify();
            });
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(50))
                    .await;
                let keep = this
                    .update(cx, |this, cx| {
                        let mut test = this.mic_test.lock();
                        if test.generation() != generation || test.state() == State::Closed {
                            return false;
                        }
                        let active = matches!(test.state(), State::Recording | State::Playing);
                        test.tick(Instant::now());
                        if active {
                            cx.notify();
                        }
                        true
                    })
                    .unwrap_or(false);
                if !keep {
                    break;
                }
            }
        }));
        cx.notify();
    }

    pub(super) fn render_mic_test(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let p = cx.palette();
        let test = self.mic_test.lock();
        let state = test.state();
        let silent = test.silent();
        let duration = test.duration();
        let remaining = test.remaining(Instant::now()).as_secs_f64().ceil() as u64;
        let level = test.level();
        let playback = test.progress();
        drop(test);
        let mut controls = div().flex().items_center().gap(px(10.));
        match state {
            State::Recording => {
                controls = controls
                    .child(crate::ui::pill::microphone_level_meter(
                        level,
                        cx.reduce_motion(),
                        &p,
                    ))
                    .child(
                        div()
                            .font_family(FONT_MONO)
                            .font_features(super::super::theme::mono_features())
                            .text_size(px(TEXT_MONO_SMALL))
                            .child(format!("0:{remaining:02}")),
                    )
                    .child(
                        Button::new("mic-test-stop")
                            .label(t("microphoneTest.stop"))
                            .variant(ButtonVariant::Outline)
                            .size(ButtonSize::Sm)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.mic_test.lock().stop();
                                cx.notify();
                            })),
                    );
            }
            State::Ready | State::Playing => {
                let playing = state == State::Playing;
                controls = controls
                    .child(
                        Button::new("mic-test-play")
                            .icon(if playing { "square" } else { "play" }, 12.)
                            .label(t(if playing {
                                "microphoneTest.stop"
                            } else {
                                "microphoneTest.play"
                            }))
                            .variant(ButtonVariant::Outline)
                            .size(ButtonSize::Sm)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.mic_test.lock().toggle_playback();
                                cx.notify();
                            })),
                    )
                    // The track takes the free width, which also parks Close at
                    // the strip's trailing edge, under the device picker.
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(48.))
                            .h(px(2.))
                            .rounded_full()
                            .bg(p.border)
                            .child(div().w(relative(playback)).h_full().bg(p.muted_foreground)),
                    )
                    .child(
                        div()
                            .font_family(FONT_MONO)
                            .text_size(px(TEXT_MONO_SMALL))
                            .child(format!("0:{:02}", duration.as_secs_f64().ceil() as u64)),
                    )
                    .child(
                        Button::new("mic-test-again")
                            .label(t("microphoneTest.recordAgain"))
                            .variant(ButtonVariant::Ghost)
                            .size(ButtonSize::Sm)
                            .on_click(cx.listener(|this, _, _, cx| this.start_mic_test(cx))),
                    )
                    .child(
                        Button::new("mic-test-close")
                            .icon("x", 12.)
                            .aria_label(t("microphoneTest.close"))
                            .variant(ButtonVariant::Ghost)
                            .size(ButtonSize::Sm)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.cancel_mic_test();
                                cx.notify();
                            })),
                    );
            }
            State::Error(problem) => {
                controls = controls
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(TEXT_SMALL))
                            .text_color(p.warning)
                            .child(t(match problem {
                                Problem::Permission => "microphoneTest.permission",
                                Problem::Device => "microphoneTest.device",
                                Problem::NoSignal => "microphoneTest.noSignal",
                                Problem::Output => "microphoneTest.output",
                            })),
                    )
                    .child(
                        Button::new("mic-test-retry")
                            .label(t("microphoneTest.tryAgain"))
                            .variant(ButtonVariant::Outline)
                            .size(ButtonSize::Sm)
                            .on_click(cx.listener(|this, _, _, cx| this.start_mic_test(cx))),
                    );
            }
            State::Closed => {}
        }
        panel_row()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(controls)
                    .when(
                        silent && matches!(state, State::Ready | State::Playing),
                        |d| {
                            d.child(
                                div()
                                    .text_size(px(TEXT_SMALL))
                                    .text_color(p.warning)
                                    .child(t("microphoneTest.silent")),
                            )
                        },
                    )
                    .child(
                        div()
                            .text_size(px(TEXT_CAPTION))
                            .line_height(px(LINE_SMALL))
                            .text_color(p.subtle_foreground)
                            .child(t("microphoneTest.privacy")),
                    ),
            )
            .with_animation("mic-test-expand", motion(DUR_BASE, cx), |d, t| {
                d.opacity(t).max_h(px(140. * ease_out(t))).overflow_hidden()
            })
            .into_any_element()
    }
}

/// Serializes timed and manual samples. Dropping the owner cancels this
/// future, so an in-flight scan can never publish into a closed sheet.
pub(super) async fn refresh_microphones<Sample, Scan, Scanning, Publish, Wait, Waiting>(
    requests: async_channel::Receiver<()>,
    mut scan: Scan,
    mut publish: Publish,
    mut wait: Wait,
) where
    Scan: FnMut() -> Scanning,
    Scanning: Future<Output = Sample>,
    Publish: FnMut(Sample) -> bool,
    Wait: FnMut() -> Waiting,
    Waiting: Future<Output = ()>,
{
    loop {
        while requests.try_recv().is_ok() {}
        if !publish(scan().await) {
            break;
        }
        let delay = wait();
        let request = requests.recv();
        futures::pin_mut!(delay, request);
        if let futures::future::Either::Right((Err(_), _)) =
            futures::future::select(delay, request).await
        {
            break;
        }
    }
}

/// Idle shows the next selection; a live recording always reports its own
/// session, even when the picker, lid, or system default changes meanwhile.
pub(super) fn microphone_status_info(
    snapshot: &MicSnapshot,
    settings: &Settings,
    capture: &CaptureStatus,
) -> (&'static str, String) {
    match capture {
        CaptureStatus::Recording(device) => ("microphoneSettings.currentlyUsing", device.clone()),
        CaptureStatus::Starting => (
            "microphoneSettings.currentlyUsing",
            t("microphoneSettings.connecting"),
        ),
        CaptureStatus::Unavailable => (
            "microphoneSettings.currentlyUsing",
            t("microphoneSettings.unknownDevice"),
        ),
        CaptureStatus::Idle => (
            "microphoneSettings.nextRecordingUses",
            snapshot
                .next_device(settings)
                .map(|d| d.label)
                .unwrap_or_else(|| t("microphoneSettings.systemDefault")),
        ),
    }
}
