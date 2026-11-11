//! Prompt studio behavior and presentation for SettingsModal.

use super::*;

fn translated_default_draft(draft: &str, before: &str, after: &str) -> Option<&'static str> {
    (draft == default_cleanup_prompt(before)).then(|| default_cleanup_prompt(after))
}

impl SettingsModal {
    pub(super) fn sync_language(
        &mut self,
        before: &str,
        after: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.prompt_editor.update(cx, |input, cx| {
            if let Some(prompt) = translated_default_draft(input.value().as_ref(), before, after) {
                input.set_value(prompt, window, cx);
            }
            input.set_placeholder(t("promptStudio.edit.placeholder"), window, cx);
        });
        self.test_input.update(cx, |input, cx| {
            input.set_placeholder(t("promptStudio.test.inputPlaceholder"), window, cx);
        });
        self.api_key_input.update(cx, |input, cx| {
            input.set_placeholder(t("apiKeyInput.placeholder"), window, cx);
        });
        for (select, key) in [
            (&self.ui_language, "settings.language.uiLabel"),
            (&self.stt_language, "settings.language.transcriptionLabel"),
            (&self.chinese_script, "settings.language.chineseScriptLabel"),
            (
                &self.pill_position,
                "settings.general.floatingIcon.startPosition",
            ),
            (&self.audio_retention, "settings.privacy.keepAudioFor"),
            (
                &self.transcript_retention,
                "settings.privacy.transcriptRetention",
            ),
            (&self.microphone, "microphoneSettings.inputDevice"),
            (&self.cleanup_model, "common.model"),
        ] {
            select.update(cx, |select, cx| select.refresh_labels(t(key), window, cx));
        }
        self.cleanup_model.update(cx, |select, _| {
            select.set_placeholder(t("reasoning.custom.modelPlaceholder"));
        });
    }

    pub(super) fn default_prompt(cx: &App) -> String {
        default_cleanup_prompt(&settings(cx).ui_language).to_string()
    }

    /// Saving is routine, so it confirms with a toast rather than a dialog
    /// to dismiss.
    pub(super) fn save_prompt(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let edited = self.prompt_editor.read(cx).value().to_string();
        let default = Self::default_prompt(cx);
        // Keep the shipped default eligible for future prompt updates.
        let custom = if edited == default {
            String::new()
        } else if edited.trim().is_empty() {
            // A blank prompt means the default: show it, so the editor
            // matches what dictation uses and follows the language.
            self.prompt_editor
                .update(cx, |input, cx| input.set_value(default, window, cx));
            String::new()
        } else {
            edited
        };
        update_settings(cx, |s| s.custom_prompt_cleanup = custom);
        show_toast(
            Toast::new(t("promptStudio.dialogs.saved.title"))
                .description(t("promptStudio.dialogs.saved.description"))
                .variant(ToastVariant::Success),
            cx,
        );
    }

    /// Resetting discards the custom prompt, so it asks first.
    pub(super) fn confirm_reset_prompt(&mut self, cx: &mut Context<Self>) {
        let this = cx.entity().downgrade();
        let mut request = DialogRequest::confirm(
            t("promptStudio.dialogs.reset.title"),
            Some(t("promptStudio.dialogs.reset.description").into()),
            true,
            move |window, cx| {
                let _ = this.update(cx, |this, cx| {
                    let default = Self::default_prompt(cx);
                    this.prompt_editor
                        .update(cx, |input, cx| input.set_value(default, window, cx));
                });
                update_settings(cx, |s| s.custom_prompt_cleanup = String::new());
                show_toast(
                    Toast::new(t("promptStudio.dialogs.reset.done")).variant(ToastVariant::Success),
                    cx,
                );
            },
        );
        request.confirm_label = t("promptStudio.dialogs.reset.confirm").into();
        show_dialog(request, cx);
    }

    pub(super) fn copy_prompt(&mut self, text: String, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        self.copied_prompt = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_secs(2)).await;
            let _ = this.update(cx, |this, cx| {
                this.copied_prompt = false;
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn run_prompt_test(&mut self, cx: &mut Context<Self>) {
        let input = self.test_input.read(cx).value().to_string();
        if input.trim().is_empty() || self.test_loading {
            return;
        }
        let s = settings(cx);
        self.test_result.clear();
        if s.cleanup_remote_url.trim().is_empty() {
            self.test_result = t("promptStudio.test.noEndpoint");
            cx.notify();
            return;
        }
        if s.cleanup_model.trim().is_empty() {
            self.test_result = t("promptStudio.test.noModelSelected");
            cx.notify();
            return;
        }
        let edited = self.prompt_editor.read(cx).value().to_string();
        let template = if edited.trim().is_empty() {
            Self::default_prompt(cx)
        } else {
            edited
        };
        self.test_loading = true;
        cx.notify();
        let hints = {
            let state = AppState::global(cx).read(cx);
            crate::text::snippets::get_dictionary_hint_words(&state.dictionary, &state.snippets)
        };
        let request = backend::run_cleanup_test(s, self.api_key.clone(), template, input, hints);
        cx.spawn(async move |this, cx| {
            let result = request.await;
            let _ = this.update(cx, |this, cx| {
                this.test_loading = false;
                this.test_result = match result {
                    Ok(text) => text,
                    Err(error) => t_with("promptStudio.test.failed", &[("error", &error)]),
                };
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn render_prompt_studio(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let p = cx.palette();
        let s = settings(cx);
        let default_prompt = Self::default_prompt(cx);
        let is_custom = !s.custom_prompt_cleanup.is_empty();
        let current_prompt = if is_custom {
            s.custom_prompt_cleanup.clone()
        } else {
            default_prompt
        };
        let tab = self.prompt_tab;
        let divider = if p.dark {
            p.border_subtle
        } else {
            p.border.opacity(0.6)
        };

        // The same segmented switcher as Theme and the server presets.
        let mut track = segmented_track(&p).gap(px(2.));
        for (value, icon_name, key) in [
            (PromptTab::View, "eye", "promptStudio.tabs.view"),
            (PromptTab::Edit, "edit3", "promptStudio.tabs.customize"),
            (PromptTab::Test, "test-tube", "promptStudio.tabs.test"),
        ] {
            let active = tab == value;
            track = track.child(
                segment(
                    SharedString::from(format!("settings-prompt-tab-{key}")),
                    t(key),
                    active,
                    cx,
                )
                .gap(px(6.))
                .child(icon(
                    icon_name,
                    14.,
                    if active {
                        p.foreground
                    } else {
                        p.muted_foreground
                    },
                ))
                .child(t(key))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.prompt_tab = value;
                    cx.notify();
                })),
            );
        }
        let tabs = div()
            .flex()
            .px(px(20.))
            .py(px(12.))
            .border_b_1()
            .border_color(divider)
            .child(track);

        let body = match tab {
            PromptTab::View => {
                let copied = self.copied_prompt;
                let text = current_prompt.replace("{{agentName}}", crate::text::DEFAULT_AGENT_NAME);
                let copy_text = current_prompt.clone();
                self.prompt_preview
                    .update(cx, |state, cx| state.set_text(&text, cx));
                let prompt_list = self.prompt_preview.read(cx).list_state().clone();
                let fade_list = prompt_list.clone();
                let preview_surface = p.card.blend(if p.dark {
                    p.surface_raised.opacity(0.3)
                } else {
                    p.muted.opacity(0.3)
                });
                div()
                    .px(px(20.))
                    .py(px(16.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .mb(px(12.))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    .child(
                                        div()
                                            .text_size(px(TEXT_SMALL))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(p.subtle_foreground)
                                            .child(if is_custom {
                                                t("promptStudio.view.customPrompt")
                                            } else {
                                                t("promptStudio.view.defaultPrompt")
                                            }),
                                    )
                                    .when(is_custom, |d| {
                                        d.child(
                                            div()
                                                .px(px(6.))
                                                .rounded_full()
                                                .bg(p.hover_wash(0.08))
                                                .text_size(px(TEXT_CAPTION))
                                                .font_weight(FontWeight::MEDIUM)
                                                .text_color(p.muted_foreground)
                                                .child(t("promptStudio.view.modified")),
                                        )
                                    }),
                            )
                            .child(
                                Button::new("copy-prompt")
                                    .icon(if copied { "check" } else { "copy" }, 12.)
                                    .label(if copied {
                                        t("promptStudio.common.copied")
                                    } else {
                                        t("promptStudio.common.copy")
                                    })
                                    .variant(ButtonVariant::Ghost)
                                    .size(ButtonSize::Sm)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.copy_prompt(copy_text.clone(), cx)
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .id("prompt-view")
                            .relative()
                            .h(px(crate::ui::theme::PROMPT_PREVIEW_HEIGHT))
                            .overflow_hidden()
                            // Bypass the List's frame-relative delta accumulation so
                            // a reversal after an edge takes effect on this event.
                            .child(wheel_region(
                                DispatchPhase::Capture,
                                None,
                                cx.listener(move |this, event: &ScrollWheelEvent, _, cx| {
                                    let offset = prompt_list.scroll_px_offset_for_scrollbar().y;
                                    let (next, remaining) = consume_delta(
                                        offset,
                                        prompt_list.max_offset_for_scrollbar().y,
                                        event.delta.pixel_delta(px(20.)).y,
                                    );
                                    prompt_list.set_offset_from_scrollbar(point(px(0.), next));
                                    let page_changed = this.scroll_page(remaining);
                                    cx.stop_propagation();
                                    if next != offset || page_changed {
                                        cx.notify();
                                    }
                                }),
                            ))
                            .p(px(16.))
                            .rounded(px(RADIUS_LG))
                            .border_1()
                            .border_color(p.border.opacity(0.7))
                            .bg(preview_surface)
                            .text_size(px(TEXT_BODY))
                            .line_height(px(LINE_READING))
                            .text_color(p.foreground)
                            .child(
                                TextView::new(&self.prompt_preview)
                                    .selectable(true)
                                    .scrollable(true)
                                    .size_full(),
                            )
                            .child(
                                canvas(
                                    |bounds, _, _| bounds,
                                    move |_, bounds, window, _| {
                                        let offset = fade_list.scroll_px_offset_for_scrollbar().y;
                                        let max = fade_list.max_offset_for_scrollbar().y;
                                        let height = px(crate::ui::theme::SCROLL_FADE_HEIGHT);
                                        for (show, top) in [
                                            (offset < -px(1.), true),
                                            (-offset < max - px(1.), false),
                                        ] {
                                            if show {
                                                let origin = if top {
                                                    bounds.origin
                                                } else {
                                                    point(bounds.origin.x, bounds.bottom() - height)
                                                };
                                                window.paint_quad(fill(
                                                    Bounds::new(
                                                        origin,
                                                        size(bounds.size.width, height),
                                                    ),
                                                    linear_gradient(
                                                        if top { 180. } else { 0. },
                                                        linear_color_stop(preview_surface, 0.),
                                                        linear_color_stop(
                                                            preview_surface.opacity(0.),
                                                            1.,
                                                        ),
                                                    ),
                                                ));
                                            }
                                        }
                                    },
                                )
                                .absolute()
                                .inset(px(16.)),
                            ),
                    )
                    .into_any_element()
            }
            PromptTab::Edit => div()
                .flex()
                .flex_col()
                .child(
                    div()
                        .px(px(20.))
                        .py(px(16.))
                        .border_b_1()
                        .border_color(divider)
                        .flex()
                        .flex_wrap()
                        .gap(px(4.))
                        .text_size(px(TEXT_SMALL))
                        .text_color(p.muted_foreground)
                        .child(
                            div()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(p.warning)
                                .child(t("promptStudio.edit.cautionLabel")),
                        )
                        .child(t("promptStudio.edit.cautionTextPrefix")),
                )
                .child(
                    div()
                        .px(px(20.))
                        .py(px(16.))
                        .border_b_1()
                        .border_color(divider)
                        .child(textarea_field(&self.prompt_editor, 360., true, window, cx)),
                )
                .child(
                    div()
                        .px(px(20.))
                        .py(px(16.))
                        .flex()
                        .gap(px(8.))
                        .child(
                            Button::new("save-prompt")
                                .icon("save", 14.)
                                .label(t("promptStudio.common.save"))
                                .size(ButtonSize::Sm)
                                .full_width()
                                .on_click(
                                    cx.listener(|this, _, window, cx| this.save_prompt(window, cx)),
                                ),
                        )
                        .child(
                            Button::new("reset-prompt")
                                .icon("rotate-ccw", 14.)
                                .label(t("promptStudio.common.reset"))
                                .variant(ButtonVariant::Outline)
                                .size(ButtonSize::Sm)
                                .on_click(
                                    cx.listener(|this, _, _, cx| this.confirm_reset_prompt(cx)),
                                ),
                        ),
                )
                .into_any_element(),
            PromptTab::Test => {
                let use_cleanup = s.use_cleanup_model;
                let disabled = self.test_input.read(cx).value().trim().is_empty()
                    || self.test_loading
                    || !use_cleanup;
                let result = self.test_result.clone();
                let row = || {
                    div()
                        .px(px(20.))
                        .py(px(16.))
                        .border_b_1()
                        .border_color(divider)
                };
                div()
                    .flex()
                    .flex_col()
                    .when(!use_cleanup, |d| {
                        d.child(
                            row().child(
                                div()
                                    .flex()
                                    .items_start()
                                    .gap(px(10.))
                                    .px(px(16.))
                                    .py(px(12.))
                                    .rounded(px(RADIUS_LG))
                                    .border_1()
                                    .border_color(p.warning.opacity(0.2))
                                    .bg(p.warning.opacity(if p.dark { 0.1 } else { 0.05 }))
                                    .child(icon("alert-triangle", 14., p.warning))
                                    .child(
                                        div()
                                            .text_size(px(TEXT_SMALL))
                                            .text_color(p.muted_foreground)
                                            .child(t("promptStudio.test.disabledReasoning")),
                                    ),
                            ),
                        )
                    })
                    .child(
                        row()
                            .flex()
                            .flex_col()
                            .gap(px(8.))
                            .child(info_line(
                                t("settings.selfHosted.serverUrl"),
                                if s.cleanup_remote_url.trim().is_empty() {
                                    t("promptStudio.test.none")
                                } else {
                                    s.cleanup_remote_url.trim().to_string()
                                },
                                &p,
                            ))
                            .child(info_line(
                                t("promptStudio.test.modelLabel"),
                                if s.cleanup_model.trim().is_empty() {
                                    t("promptStudio.test.none")
                                } else {
                                    s.cleanup_model.trim().to_string()
                                },
                                &p,
                            )),
                    )
                    .child(
                        row()
                            .child(
                                div()
                                    .mb(px(8.))
                                    .text_size(px(TEXT_SMALL))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(p.foreground)
                                    .child(t("promptStudio.test.inputLabel")),
                            )
                            .child(textarea_field(&self.test_input, 84., false, window, cx)),
                    )
                    .child(
                        row().child(
                            Button::new("run-prompt-test")
                                .icon("play", 14.)
                                .label(if self.test_loading {
                                    t("promptStudio.test.processing")
                                } else {
                                    t("promptStudio.test.run")
                                })
                                .size(ButtonSize::Sm)
                                .full_width()
                                .disabled(disabled)
                                .on_click(cx.listener(|this, _, _, cx| this.run_prompt_test(cx))),
                        ),
                    )
                    .when(!result.is_empty(), |d| {
                        let copy_text = result.clone();
                        d.child(
                            div()
                                .px(px(20.))
                                .py(px(16.))
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .justify_between()
                                        .mb(px(8.))
                                        .child(
                                            div()
                                                .text_size(px(TEXT_SMALL))
                                                .font_weight(FontWeight::MEDIUM)
                                                .text_color(p.foreground)
                                                .child(t("promptStudio.test.outputLabel")),
                                        )
                                        .child(
                                            Button::new("settings-copy-test-output")
                                                .icon("copy", 12.)
                                                .aria_label(t("promptStudio.common.copy"))
                                                .variant(ButtonVariant::Ghost)
                                                .size(ButtonSize::Sm)
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    this.copy_prompt(copy_text.clone(), cx)
                                                })),
                                        ),
                                )
                                .child(
                                    div()
                                        .id("test-output")
                                        .max_h(px(192.))
                                        .overflow_y_scroll()
                                        .p(px(16.))
                                        .rounded(px(RADIUS_LG))
                                        .border_1()
                                        .border_color(p.border.opacity(0.7))
                                        .bg(if p.dark {
                                            p.surface_raised.opacity(0.3)
                                        } else {
                                            p.muted.opacity(0.3)
                                        })
                                        .text_size(px(TEXT_SMALL))
                                        .line_height(px(LINE_SMALL))
                                        .text_color(p.foreground)
                                        .child(result),
                                ),
                        )
                    })
                    .into_any_element()
            }
        };

        div()
            .rounded(px(RADIUS_XL))
            .border_1()
            .border_color(if p.dark {
                p.border_subtle
            } else {
                p.border.opacity(0.7)
            })
            .bg(p.card)
            .overflow_hidden()
            .child(tabs)
            .child(body)
    }
}

#[cfg(test)]
mod tests {
    use super::{default_cleanup_prompt, translated_default_draft};

    #[test]
    fn language_switch_translates_only_the_unchanged_default() {
        let en = default_cleanup_prompt("en");
        let zh = default_cleanup_prompt("zh-CN");
        assert_ne!(en, zh);
        assert_eq!(translated_default_draft(en, "en", "zh-CN"), Some(zh));
        assert_eq!(translated_default_draft(zh, "zh-CN", "en"), Some(en));
        assert_eq!(
            translated_default_draft("My custom prompt", "en", "zh-CN"),
            None
        );
        assert_eq!(
            translated_default_draft(&format!("{en}\n"), "en", "zh-CN"),
            None
        );
        assert_eq!(translated_default_draft("", "en", "zh-CN"), None);
    }
}
