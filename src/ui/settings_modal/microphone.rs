//! Microphone behavior and presentation for SettingsModal.

use super::*;

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
                    // Refresh sits beside the list it reloads.
                    .child(
                        div()
                            .flex_none()
                            .flex()
                            .items_center()
                            .gap(px(6.))
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
                            )
                            .child(self.microphone.clone()),
                    ),
            )
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
