//! Privacy behavior and presentation for SettingsModal.

use super::*;

impl SettingsModal {
    pub(super) fn refresh_permissions(&mut self, cx: &mut Context<Self>) {
        self.mic_permission = backend::microphone_permission_granted();
        self.accessibility_permission = backend::accessibility_granted();
        cx.notify();
    }

    fn request_microphone_permission(&mut self, cx: &mut Context<Self>) {
        if self.permission_request.is_some() {
            return;
        }
        self.permission_request = Some(cx.spawn(async move |this, cx| {
            cx.background_spawn(async { backend::request_microphone_permission() })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.permission_request = None;
                this.refresh_permissions(cx);
            });
        }));
    }

    /// Coalesces scans and never publishes a result made stale by deletion.
    pub(super) fn load_storage_usage(&mut self, cx: &mut Context<Self>) {
        self.storage_usage_generation = self.storage_usage_generation.wrapping_add(1);
        if self.storage_usage_loading {
            return;
        }
        self.storage_usage_loading = true;
        cx.spawn(async move |this, cx| {
            loop {
                let Ok(generation) = this.update(cx, |this, _| this.storage_usage_generation)
                else {
                    return;
                };
                let usage = cx
                    .background_spawn(async { backend::audio_storage_usage() })
                    .await;
                let retry = this.update(cx, |this, cx| {
                    if generation != this.storage_usage_generation {
                        return true;
                    }
                    this.storage_usage = usage;
                    this.storage_usage_loading = false;
                    cx.notify();
                    false
                });
                if !matches!(retry, Ok(true)) {
                    break;
                }
            }
        })
        .detach();
    }

    pub(super) fn render_privacy(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let p = cx.palette();
        let s = settings(cx);
        let (files, bytes) = self.storage_usage;
        let usage = if self.storage_usage_loading || self.clearing_audio {
            "…".to_string()
        } else if files > 0 {
            t_with(
                "settings.privacy.audioStorageFiles",
                &[
                    ("count", &files.to_string()),
                    ("size", &crate::text::format::format_bytes(bytes)),
                ],
            )
        } else if !s.data_retention_enabled || s.audio_retention_days == 0 {
            // Says why Clear All Audio is unavailable and how to change it.
            t("settings.privacy.audioStorageOff")
        } else {
            t("settings.privacy.audioStorageEmpty")
        };
        let history_on = s.data_retention_enabled;
        div()
            .flex()
            .flex_col()
            .gap(px(24.))
            // History comes first: audio is only kept for saved dictations.
            .child(settings_section(
                t("settings.privacy.historyTitle"),
                vec![
                    toggle_row(
                        "data-retention",
                        t("settings.privacy.dataRetention"),
                        Some(t("settings.privacy.dataRetentionDescription")),
                        history_on,
                        false,
                        |v, _, cx| update_settings(cx, |s| s.data_retention_enabled = v),
                        &p,
                    ),
                    settings_row(
                        t("settings.privacy.transcriptRetention"),
                        Some(
                            if history_on {
                                t("settings.privacy.transcriptRetentionDescription")
                            } else {
                                t("settings.privacy.needsHistory")
                            }
                            .into(),
                        ),
                        self.transcript_retention.clone(),
                        &p,
                    )
                    .into_any_element(),
                ],
                &p,
            ))
            .child(settings_section(
                t("settings.privacy.audioRetention"),
                vec![
                    settings_row(
                        t("settings.privacy.keepAudioFor"),
                        Some(
                            if history_on {
                                t("settings.privacy.keepAudioForDescription")
                            } else {
                                t("settings.privacy.needsHistory")
                            }
                            .into(),
                        ),
                        self.audio_retention.clone(),
                        &p,
                    )
                    .into_any_element(),
                    settings_row(
                        t("settings.privacy.audioStorageUsage"),
                        Some(usage.into()),
                        Button::new("settings-clear-audio")
                            .label(t("settings.privacy.clearAllAudio"))
                            .variant(ButtonVariant::Outline)
                            .size(ButtonSize::Sm)
                            .disabled(
                                files == 0 || self.storage_usage_loading || self.clearing_audio,
                            )
                            .on_click(cx.listener(|this, _, _, cx| this.confirm_clear_audio(cx))),
                        &p,
                    )
                    .into_any_element(),
                ],
                &p,
            ))
            .child(settings_section(
                t("settings.permissions.title"),
                vec![
                    permission_row(
                        "settings-perm-mic",
                        t("settings.permissions.microphoneTitle"),
                        t("settings.permissions.microphoneDescription"),
                        self.mic_permission,
                        cx.listener(|this, _, _, cx| {
                            this.request_microphone_permission(cx);
                        }),
                        &p,
                    ),
                    permission_row(
                        "settings-perm-ax",
                        t("settings.permissions.accessibilityTitle"),
                        t("settings.permissions.accessibilityDescription"),
                        self.accessibility_permission,
                        cx.listener(|this, _, _, cx| {
                            backend::request_accessibility_permission();
                            this.accessibility_permission = backend::accessibility_granted();
                            cx.notify();
                        }),
                        &p,
                    ),
                ],
                &p,
            ))
            .child(settings_section(
                t("settings.permissions.troubleshootingTitle"),
                vec![
                    settings_row(
                        t("settings.permissions.resetAccessibility.label"),
                        Some(t("settings.permissions.resetAccessibility.rowDescription").into()),
                        Button::new("settings-troubleshoot-ax")
                            .label(t("settings.permissions.resetAccessibility.action"))
                            .variant(ButtonVariant::Outline)
                            .size(ButtonSize::Sm)
                            .on_click(
                                cx.listener(|this, _, _, cx| this.confirm_reset_accessibility(cx)),
                            ),
                        &p,
                    )
                    .into_any_element(),
                ],
                &p,
            ))
            .into_any_element()
    }

    /// Removes Whisper's Accessibility entry so macOS can grant it afresh,
    /// the fix when a stale grant from an earlier build blocks auto-paste.
    /// When the entry can't be removed, System Settings opens instead and the
    /// toast says what to do there, so the confirmation never ends in silence.
    pub(super) fn confirm_reset_accessibility(&mut self, cx: &mut Context<Self>) {
        let this = cx.entity().downgrade();
        let mut request = DialogRequest::confirm(
            t("settings.permissions.resetAccessibility.title"),
            Some(t("settings.permissions.resetAccessibility.description").into()),
            true,
            move |_, cx| {
                let this = this.clone();
                let reset = cx.background_spawn(async { backend::reset_accessibility_entry() });
                cx.spawn(async move |cx| {
                    let removed = reset.await;
                    cx.update(|cx| {
                        let toast = if removed {
                            backend::request_accessibility_permission();
                            Toast::new(t("settings.permissions.resetAccessibility.done"))
                                .description(t(
                                    "settings.permissions.resetAccessibility.doneDescription",
                                ))
                                .variant(ToastVariant::Success)
                        } else {
                            backend::open_accessibility_settings();
                            Toast::new(t("settings.permissions.resetAccessibility.failed"))
                                .description(t(
                                    "settings.permissions.resetAccessibility.failedDescription",
                                ))
                                .variant(ToastVariant::Destructive)
                        };
                        show_toast(toast, cx);
                        let _ = this.update(cx, |this, cx| {
                            this.accessibility_permission = backend::accessibility_granted();
                            cx.notify();
                        });
                    });
                })
                .detach();
            },
        );
        request.confirm_label = t("settings.permissions.resetAccessibility.confirm").into();
        show_dialog(request, cx);
    }

    /// Deleting every recording cannot be undone, so it asks first and then
    /// reports what was freed.
    pub(super) fn confirm_clear_audio(&mut self, cx: &mut Context<Self>) {
        let (files, bytes) = self.storage_usage;
        let size = crate::text::format::format_bytes(bytes);
        let this = cx.entity().downgrade();
        let mut request = DialogRequest::confirm(
            t("settings.privacy.clearAudioTitle"),
            Some(
                t_with(
                    "settings.privacy.clearAudioDescription",
                    &[("count", &files.to_string()), ("size", &size)],
                )
                .into(),
            ),
            true,
            move |_, cx| {
                let clear = AppState::global(cx).update(cx, |s, cx| s.delete_all_audio(cx));
                let _ = this.update(cx, |this, cx| {
                    this.clearing_audio = true;
                    cx.notify();
                });
                let this = this.clone();
                let size = size.clone();
                cx.spawn(async move |cx| {
                    clear.await;
                    cx.update(|cx| {
                        let _ = this.update(cx, |this, cx| {
                            this.clearing_audio = false;
                            this.load_storage_usage(cx);
                            cx.notify();
                        });
                        show_toast(
                            Toast::new(t_with(
                                "settings.privacy.audioCleared",
                                &[("count", &files.to_string()), ("size", &size)],
                            ))
                            .variant(ToastVariant::Success),
                            cx,
                        );
                    });
                })
                .detach();
            },
        );
        request.confirm_label = t("settings.privacy.clearAudioConfirm").into();
        show_dialog(request, cx);
    }
}
