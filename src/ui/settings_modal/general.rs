//! General behavior and presentation for SettingsModal.

use super::*;
use crate::ui::components::label_block;

impl SettingsModal {
    pub(super) fn render_general(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let p = cx.palette();
        let s = settings(cx);

        let theme_switch = {
            let mut track = segmented_track(&p).items_center().flex_wrap().gap(px(2.));
            for (value, icon_name, key) in [
                (
                    ThemePreference::Light,
                    "sun",
                    "settings.general.appearance.light",
                ),
                (
                    ThemePreference::Dark,
                    "moon",
                    "settings.general.appearance.dark",
                ),
                (
                    ThemePreference::TokyoNight,
                    "moon",
                    "settings.general.appearance.tokyoNight",
                ),
                (
                    ThemePreference::Auto,
                    "monitor",
                    "settings.general.appearance.auto",
                ),
            ] {
                let selected = s.theme == value;
                track = track.child(
                    segment(
                        SharedString::from(format!("theme-{key}")),
                        t(key),
                        selected,
                        cx,
                    )
                    .gap(px(4.))
                    .child(icon(
                        icon_name,
                        14.,
                        if selected {
                            p.foreground
                        } else {
                            p.muted_foreground
                        },
                    ))
                    .child(t(key))
                    .on_click(move |_, _, cx| update_settings(cx, |s| s.theme = value)),
                );
            }
            track
        };

        let toggle = |id: &'static str,
                      label: &str,
                      description: &str,
                      checked: bool,
                      f: fn(&mut Settings, bool)| {
            toggle_row(
                id,
                t(label),
                Some(t(description)),
                checked,
                false,
                move |value, _, cx| update_settings(cx, |s| f(s, value)),
                &p,
            )
        };

        let auto_start = self.auto_start;
        let mut startup_rows = vec![toggle_row(
            "launch-at-login",
            t("settings.general.startup.launchAtLogin"),
            Some(t("settings.general.startup.launchAtLoginDescription")),
            auto_start.enabled,
            self.auto_start_loading,
            {
                let this = cx.entity().downgrade();
                move |enabled, _, cx| {
                    if let Err(err) = backend::set_auto_start(enabled) {
                        log::warn!("failed to change launch at login: {err}");
                    }
                    let _ = this.update(cx, |this, cx| {
                        this.auto_start = backend::auto_start_state();
                        cx.notify();
                    });
                }
            },
            &p,
        )];
        if auto_start.requires_approval {
            startup_rows.push(
                panel_row()
                    .child(
                        div()
                            .flex()
                            .gap(px(12.))
                            .p(px(16.))
                            .rounded(px(RADIUS_LG))
                            .border_1()
                            .border_color(p.warning.opacity(0.3))
                            .bg(p.warning.opacity(if p.dark { 0.1 } else { 0.05 }))
                            .child(icon("alert-triangle", 16., p.warning))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(8.))
                                    .child(
                                        div()
                                            .text_size(px(TEXT_BODY))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(p.foreground)
                                            .child(t("settings.general.startup.needsApproval.title")),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(TEXT_SMALL))
                                            .line_height(px(LINE_SMALL))
                                            .text_color(p.muted_foreground)
                                            .child(t("settings.general.startup.needsApproval.description")),
                                    )
                                    .child(
                                        Button::new("open-login-items")
                                            .label(t("settings.general.startup.needsApproval.action"))
                                            .variant(ButtonVariant::Outline)
                                            .size(ButtonSize::Sm)
                                            .on_click(|_, _, _| backend::open_login_items_settings()),
                                    ),
                            ),
                    )
                    .into_any_element(),
            );
        }
        startup_rows.push(toggle(
            "start-minimized",
            "settings.general.startup.startMinimized",
            "settings.general.startup.startMinimizedDescription",
            s.start_minimized,
            |s, v| s.start_minimized = v,
        ));

        div()
            .flex()
            .flex_col()
            .gap(px(24.))
            .child(settings_section(
                t("settings.general.appearance.title"),
                vec![
                    panel_row()
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(12.))
                                .child(label_block(
                                    t("settings.general.appearance.theme"),
                                    Some(t("settings.general.appearance.themeDescription").into()),
                                    &p,
                                ))
                                .child(div().flex().child(theme_switch)),
                        )
                        .into_any_element(),
                    toggle(
                        "menu-bar-icon",
                        "settings.general.appearance.showMenuBarIcon",
                        "settings.general.appearance.showMenuBarIconDescription",
                        s.show_menu_bar_icon,
                        |s, v| s.show_menu_bar_icon = v,
                    ),
                ],
                &p,
            ))
            .child(settings_section(
                t("settings.language.sectionTitle"),
                vec![self.render_ui_language(cx)],
                &p,
            ))
            .child(settings_section(
                t("settings.general.soundEffects.title"),
                vec![
                    toggle(
                        "audio-cues",
                        "settings.general.soundEffects.dictationSounds",
                        "settings.general.soundEffects.dictationSoundsDescription",
                        s.audio_cues_enabled,
                        |s, v| s.audio_cues_enabled = v,
                    ),
                    toggle(
                        "pause-media",
                        "settings.general.soundEffects.pauseMedia",
                        "settings.general.soundEffects.pauseMediaDescription",
                        s.pause_media_on_dictation,
                        |s, v| s.pause_media_on_dictation = v,
                    ),
                ],
                &p,
            ))
            .child(settings_section(
                t("settings.general.clipboard.title"),
                vec![
                    toggle(
                        "auto-paste",
                        "settings.general.clipboard.autoPaste",
                        "settings.general.clipboard.autoPasteDescription",
                        s.auto_paste_enabled,
                        |s, v| s.auto_paste_enabled = v,
                    ),
                    toggle(
                        "keep-clipboard",
                        "settings.general.clipboard.keepInClipboard",
                        "settings.general.clipboard.keepInClipboardDescription",
                        s.keep_transcription_in_clipboard,
                        |s, v| s.keep_transcription_in_clipboard = v,
                    ),
                ],
                &p,
            ))
            .child(settings_section(
                t("settings.general.startup.title"),
                startup_rows,
                &p,
            ))
            .child(settings_section(
                t("settings.dictionary.title"),
                vec![toggle(
                    "auto-learn",
                    "settings.dictionary.autoLearnTitle",
                    "settings.dictionary.autoLearnDescription",
                    s.auto_learn_corrections,
                    |s, v| s.auto_learn_corrections = v,
                )],
                &p,
            ))
            .into_any_element()
    }

    /// The interface language row. Its description points to where the
    /// spoken language lives, and that pointer is a link to the page.
    pub(super) fn render_ui_language(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let p = cx.palette();
        let link_id = "settings-ui-language-stt-link";
        let link_label = t("settings.language.uiDescriptionLink");
        let link = div()
            .id(link_id)
            .keyboard_focusable(link_id, Role::Link, link_label.clone(), cx)
            .rounded(px(RADIUS_SM))
            .cursor_pointer()
            .text_color(p.foreground)
            .hover(|s| s.underline())
            .child(link_label)
            .on_click(
                cx.listener(|this, _, _, cx| this.set_section(SettingsSection::SpeechToText, cx)),
            );
        let description = div()
            .mt(px(2.))
            .max_w(px(crate::ui::components::DESCRIPTION_MAX_WIDTH))
            .flex()
            .flex_col()
            .items_start()
            .w_full()
            .text_size(px(TEXT_SMALL))
            .line_height(px(LINE_SMALL))
            .text_color(p.subtle_foreground)
            .child(div().w_full().child(t("settings.language.uiDescription")))
            .child(link);
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
                                div()
                                    .text_size(px(TEXT_BODY))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(p.foreground)
                                    .child(t("settings.language.uiLabel")),
                            )
                            .child(description),
                    )
                    .child(div().flex_none().child(self.ui_language.clone())),
            )
            .into_any_element()
    }

    /// The row label and device name to report under the picker, when the
    /// selection resolves to a device without the user reading the dropdown.
    /// The Dictation page: everything about the act of dictating, from the
    /// keys that start it to the microphone and the pill it shows.
    pub(super) fn render_hotkeys(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = cx.palette();
        let mut microphone_rows = vec![self.render_microphone(window, cx)];
        let capture = Dictation::global(cx).read(cx).capture_status();
        let (label, device) = microphone_status_info(&self.mic_snapshot, &settings(cx), &capture);
        let healthy = matches!(capture, CaptureStatus::Recording(_) | CaptureStatus::Idle);
        let device_status = if healthy {
            status_label(device, &p)
        } else {
            div()
                .text_size(px(TEXT_SMALL))
                .text_color(p.muted_foreground)
                .child(device)
        };
        microphone_rows.push(
            settings_row(
                t(label),
                matches!(capture, CaptureStatus::Recording(_))
                    .then(|| t("microphoneSettings.appliesNextRecording").into()),
                device_status,
                &p,
            )
            .into_any_element(),
        );
        let s = settings(cx);
        let mode = s.activation_mode;
        let keys = s.effective_dictation_key();
        let selector = activation_mode_cards(
            mode,
            TapStyle::of(&parse_hotkey_list(&keys)),
            backend::push_disabled_reason(&keys),
            cx,
        );
        div()
            .flex()
            .flex_col()
            .gap(px(24.))
            .child(settings_section(
                t("settings.general.hotkey.title"),
                vec![self.hotkeys.clone().into_any_element()],
                &p,
            ))
            .child(
                div()
                    .child(
                        section_header(t("settings.general.hotkey.activationMode"), &p).px(px(16.)),
                    )
                    .child(selector),
            )
            .child(settings_section(
                t("settings.general.microphone.title"),
                microphone_rows,
                &p,
            ))
            .child(settings_section(
                t("settings.general.floatingIcon.title"),
                vec![
                    settings_row(
                        t("settings.general.floatingIcon.startPosition"),
                        Some(t("settings.general.floatingIcon.startPositionDescription").into()),
                        self.pill_position.clone(),
                        &p,
                    )
                    .into_any_element(),
                ],
                &p,
            ))
            .into_any_element()
    }
}
