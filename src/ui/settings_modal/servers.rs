//! Servers behavior and presentation for SettingsModal.

use super::*;

impl SettingsModal {
    pub(super) fn cleanup_draft(&self, cx: &App) -> String {
        self.cleanup_url.read(cx).value().trim().to_string()
    }

    pub(super) fn apply_cleanup_base_on_blur(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let draft = self.cleanup_draft(cx);
        if draft.is_empty() {
            return;
        }
        if draft != settings(cx).cleanup_remote_url.trim() {
            self.apply_cleanup_base(window, cx);
        }
    }

    /// Shortcuts that fill the Server URL with a known server's default
    /// address. They are not a selection: a custom address matches none, and
    /// the one whose address the field holds carries a check.
    pub(super) fn cleanup_presets(
        &self,
        draft: &str,
        disabled: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let current = normalize_base_url(draft);
        let mut row = div().flex().flex_wrap().gap(px(6.));
        for &(name, url) in CLEANUP_PRESETS.iter() {
            row = row.child(
                suggestion_chip(
                    SharedString::from(format!("settings-preset-{name}")),
                    name,
                    t_with("settings.llms.server.fillIn", &[("server", name)]),
                    current == url,
                    false,
                    !disabled,
                    cx,
                )
                .when(!disabled, |d| {
                    d.on_click(cx.listener(move |this, _, window, cx| {
                        this.cleanup_url
                            .update(cx, |input, cx| input.set_value(url, window, cx));
                        this.apply_cleanup_base(window, cx);
                    }))
                }),
            );
        }
        row.into_any_element()
    }

    pub(super) fn apply_cleanup_base(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let draft = self.cleanup_draft(cx);
        let normalized = if draft.is_empty() {
            draft
        } else {
            normalize_base_url(&draft)
        };
        self.cleanup_url.update(cx, |input, cx| {
            input.set_value(normalized.clone(), window, cx)
        });
        let saved = normalized.clone();
        update_settings(cx, |s| s.cleanup_remote_url = saved);
        self.last_loaded_base = None;
        self.load_models(Some(normalized), true, cx);
    }

    pub(super) fn refresh_models(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let draft = self.cleanup_draft(cx);
        if draft != settings(cx).cleanup_remote_url.trim() {
            self.apply_cleanup_base(window, cx);
            return;
        }
        if draft.is_empty() {
            return;
        }
        self.load_models(None, true, cx);
    }

    pub(super) fn load_models(
        &mut self,
        base_override: Option<String>,
        force: bool,
        cx: &mut Context<Self>,
    ) {
        let raw = base_override.unwrap_or_else(|| settings(cx).cleanup_remote_url.clone());
        let normalized = normalize_base_url(&raw);
        if normalized.is_empty() {
            self.models.clear();
            self.models_error = None;
            self.models_loading = false;
            self.last_loaded_base = None;
            cx.notify();
            return;
        }
        if !force && self.last_loaded_base.as_deref() == Some(normalized.as_str()) {
            return;
        }
        self.models_loading = true;
        self.models_error = None;
        self.models_error_details = false;
        cx.notify();
        let api_key = self.api_key.trim().to_string();
        let request = backend::list_models(normalized.clone(), api_key.clone());
        self.models_request = Some(cx.spawn(async move |this, cx| {
            let result = request.await;
            let _ = this.update(cx, |this, cx| {
                this.models_loading = false;
                match result {
                    Ok((resolved, models)) => {
                        if resolved != normalized {
                            let saved = resolved.clone();
                            update_settings(cx, |s| s.cleanup_remote_url = saved);
                        }
                        this.models = models;
                        this.last_loaded_base = Some(resolved);
                    }
                    Err(message) => {
                        this.models.clear();
                        this.models_error = Some(message);
                    }
                }
                cx.notify();
            });
        }));
    }

    pub(super) fn reset_stt_check(&mut self) {
        self.stt_request = None;
        self.stt_check = ConnectionCheck::Idle;
        self.stt_models.clear();
    }

    /// Keeps "model available" true to the field after a check, without
    /// another request, since the server's list is already known.
    pub(super) fn recheck_model_listed(&mut self, model: &str) {
        if matches!(self.stt_check, ConnectionCheck::Connected { .. }) {
            self.stt_check = ConnectionCheck::connected(&self.stt_models, model);
        }
    }

    /// Asks the server for its model list: a cheap request every
    /// OpenAI-compatible server answers, which proves the address works.
    pub(super) fn test_stt_connection(&mut self, cx: &mut Context<Self>) {
        let s = settings(cx);
        let base = normalize_base_url(&s.remote_transcription_url);
        if base.is_empty() {
            return;
        }
        let model = s.remote_transcription_model.trim().to_string();
        self.stt_check = ConnectionCheck::Checking;
        self.stt_error_details = false;
        cx.notify();
        let request = backend::list_models(base.clone(), String::new());
        self.stt_request = Some(cx.spawn(async move |this, cx| {
            let result = request.await;
            let _ = this.update(cx, |this, cx| {
                this.stt_check = match result {
                    Ok((_, models)) => {
                        this.stt_models = models;
                        ConnectionCheck::connected(&this.stt_models, &model)
                    }
                    Err(error) => ConnectionCheck::Failed(error),
                };
                cx.notify();
            });
        }));
    }

    pub(super) fn save_api_key(&mut self, cx: &mut Context<Self>) {
        let key = self.api_key_input.read(cx).value().trim().to_string();
        self.api_key = key.clone();
        self.api_key_editing = false;
        backend::set_cleanup_api_key(&key);
        cx.notify();
    }

    pub(super) fn edit_api_key(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.api_key_editing = true;
        let key = self.api_key.clone();
        self.api_key_input.update(cx, |input, cx| {
            input.set_value(key, window, cx);
            input.focus(window, cx);
            input.select_all(window, cx);
        });
        cx.notify();
    }

    /// The page heading is the breadcrumb; the panel opens with the server,
    /// titled like the Text cleanup server so the two pages read alike.
    pub(super) fn render_speech_to_text(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = cx.palette();
        let mut language_rows = vec![
            settings_row(
                t("settings.language.transcriptionLabel"),
                Some(t("settings.language.transcriptionDescription").into()),
                self.stt_language.clone(),
                &p,
            )
            .into_any_element(),
        ];
        // The script choice only applies to auto-detected Chinese: a fixed
        // zh-CN or zh-TW already names its script, and any other language
        // never produces Chinese characters.
        let auto_language = {
            let language = &settings(cx).preferred_language;
            language.is_empty() || language == "auto"
        };
        if auto_language {
            language_rows.push(
                settings_row(
                    t("settings.language.chineseScriptLabel"),
                    Some(t("settings.language.chineseScriptDescription").into()),
                    self.chinese_script.clone(),
                    &p,
                )
                .into_any_element(),
            );
        }
        let server = settings_section(
            t("settings.speechToText.serverTitle"),
            vec![
                settings_field_tagged(
                    t("settings.selfHosted.serverUrl"),
                    t("settings.speechToText.protocolTag"),
                    Some(t("settings.speechToText.serverUrlHelp").into_any_element()),
                    endpoint_control(
                        text_field(&self.stt_url, true, window, cx),
                        self.stt_url.read(cx).value().as_ref(),
                        "/audio/transcriptions",
                        &p,
                    ),
                    &p,
                )
                .into_any_element(),
                settings_field(
                    t("common.model"),
                    Some(t("settings.speechToText.modelHelp").into_any_element()),
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(8.))
                        .child(text_field(&self.stt_model, true, window, cx))
                        .when(!self.stt_models.is_empty(), |d| {
                            d.child(self.stt_model_choices(cx))
                        }),
                    &p,
                )
                .into_any_element(),
                self.render_stt_check(cx),
            ],
            &p,
        );
        div()
            .flex()
            .flex_col()
            .gap(px(24.))
            .child(server)
            .child(settings_section(
                t("settings.language.sectionTitle"),
                language_rows,
                &p,
            ))
            .into_any_element()
    }

    /// The models the server listed, as chips that fill the free-text Model
    /// field; the one matching the field carries a check.
    pub(super) fn stt_model_choices(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = cx.palette();
        let current = self.stt_model.read(cx).value().trim().to_string();
        let mut row = div().flex().flex_wrap().gap(px(6.));
        for (index, model) in self.stt_models.iter().take(STT_MODEL_CHOICES).enumerate() {
            let value = model.selected_value(&current).to_string();
            row = row.child(
                suggestion_chip(
                    ("settings-stt-model", index),
                    value.clone(),
                    value.clone(),
                    model.matches(&current),
                    true,
                    true,
                    cx,
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    let value = value.clone();
                    this.stt_model
                        .update(cx, |input, cx| input.set_value(value.clone(), window, cx));
                    this.recheck_model_listed(&value);
                    update_settings(cx, |s| s.remote_transcription_model = value);
                    cx.notify();
                })),
            );
        }
        div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(
                div()
                    .text_size(px(TEXT_SMALL))
                    .line_height(px(LINE_SMALL))
                    .text_color(p.subtle_foreground)
                    .child(t("settings.speechToText.serverModels")),
            )
            .child(row)
            .into_any_element()
    }

    pub(super) fn render_stt_check(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let s = settings(cx);
        let base = normalize_base_url(&s.remote_transcription_url);
        let status = connection_status(
            &self.stt_check,
            &base,
            "settings-stt-error-details",
            self.stt_error_details,
            cx.listener(|this, _, _, cx| {
                this.stt_error_details = !this.stt_error_details;
                cx.notify();
            }),
            cx,
        );
        connection_row(
            "settings-stt-test",
            status,
            !base.is_empty() && self.stt_check != ConnectionCheck::Checking,
            cx.listener(|this, _, _, cx| this.test_stt_connection(cx)),
            cx,
        )
    }

    /// The page opens with its master switch in a section named for what it
    /// does, not repeating the page title from the breadcrumb. The Server and Output groups dim and stop taking
    /// input while cleanup is off, so the dependency reads at a glance.
    pub(super) fn render_llms(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = cx.palette();
        let s = settings(cx);
        let enabled = s.use_cleanup_model;
        let master = settings_section(
            t("settings.llms.cleanup.title"),
            vec![toggle_row(
                "use-cleanup",
                t("settings.aiModels.enableTextCleanup"),
                Some(t("settings.aiModels.enableTextCleanupDescription")),
                enabled,
                false,
                |v, _, cx| update_settings(cx, |s| s.use_cleanup_model = v),
                &p,
            )],
            &p,
        );
        let server_rows = self.render_cleanup_server(!enabled, window, cx);
        // Reasoning shapes what the model sends back, not how Whisper
        // reaches the server, so it sits with the prompt rather than the
        // connection fields.
        let output = settings_section(
            t("settings.llms.output.title"),
            vec![toggle_row(
                "disable-thinking",
                t("reasoning.disableThinking.label"),
                Some(t("reasoning.disableThinking.help")),
                s.cleanup_disable_thinking,
                !enabled,
                |v, _, cx| update_settings(cx, |s| s.cleanup_disable_thinking = v),
                &p,
            )],
            &p,
        );
        div()
            .flex()
            .flex_col()
            .gap(px(24.))
            .child(master)
            .child(
                settings_section(t("settings.llms.server.title"), server_rows, &p)
                    .when(!enabled, |d| d.opacity(0.5)),
            )
            .child(
                div()
                    .child(section_header(t("settings.prompts.title"), &p).px(px(16.)))
                    .child(self.render_prompt_studio(window, cx)),
            )
            .child(output.when(!enabled, |d| d.opacity(0.5)))
            .into_any_element()
    }

    /// The cleanup server's Connection state, read off the model fetch that
    /// Test Connection (and applying a new URL) runs.
    pub(super) fn cleanup_check(&self, is_dirty: bool, cx: &App) -> ConnectionCheck {
        if self.models_loading {
            ConnectionCheck::Checking
        } else if let Some(error) = &self.models_error {
            ConnectionCheck::Failed(error.clone())
        } else if self.last_loaded_base.is_some() && !is_dirty {
            let model = settings(cx).cleanup_model;
            let model = model.trim();
            ConnectionCheck::Connected {
                model_listed: (!model.is_empty() && !self.models.is_empty())
                    .then(|| self.models.iter().any(|m| m.matches(model))),
            }
        } else {
            ConnectionCheck::Idle
        }
    }

    pub(super) fn render_cleanup_server(
        &mut self,
        disabled: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let p = cx.palette();
        let s = settings(cx);
        let draft = self.cleanup_draft(cx);
        let is_dirty = draft != s.cleanup_remote_url.trim();
        let saved_base = normalize_base_url(&s.cleanup_remote_url);
        let model = s.cleanup_model.trim().to_string();
        let check = if disabled {
            ConnectionCheck::Idle
        } else {
            self.cleanup_check(is_dirty, cx)
        };
        let status = connection_status(
            &check,
            &saved_base,
            "settings-models-error-details",
            self.models_error_details,
            cx.listener(|this, _, _, cx| {
                this.models_error_details = !this.models_error_details;
                cx.notify();
            }),
            cx,
        );
        let can_test =
            !disabled && !self.models_loading && !(draft.is_empty() && saved_base.is_empty());
        let api_key = self.render_api_key(disabled, window, cx);
        let models = if is_dirty || disabled {
            &[][..]
        } else {
            self.models.as_slice()
        };
        let options = cleanup_model_options(models, &model);
        self.cleanup_model.update(cx, |select, cx| {
            select.set_options(options, cx);
            select.set_selected((!model.is_empty()).then(|| model.into()), cx);
            select.set_disabled(disabled, cx);
        });

        vec![
            settings_field_tagged(
                t("settings.selfHosted.serverUrl"),
                t("settings.llms.server.protocolTag"),
                Some(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(10.))
                        .child(t("reasoning.selfHosted.endpointHelp"))
                        .child(self.cleanup_presets(&draft, disabled, cx))
                        .into_any_element(),
                ),
                endpoint_control(
                    text_field_disabled(&self.cleanup_url, true, disabled, window, cx),
                    &draft,
                    "/chat/completions",
                    &p,
                ),
                &p,
            )
            .into_any_element(),
            settings_field(
                t("reasoning.custom.apiKeyOptional"),
                Some(t("reasoning.custom.apiKeyHelp").into_any_element()),
                api_key,
                &p,
            )
            .into_any_element(),
            settings_field(
                t("common.model"),
                Some(t("reasoning.custom.modelHelp").into_any_element()),
                self.cleanup_model.clone(),
                &p,
            )
            .into_any_element(),
            panel_row()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(px(16.))
                        .child(div().flex_1().min_w_0().child(status))
                        .child(
                            Button::new("settings-cleanup-test")
                                .label(t("settings.connection.test"))
                                .variant(ButtonVariant::Outline)
                                .size(ButtonSize::Sm)
                                .disabled(!can_test)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.refresh_models(window, cx)
                                })),
                        ),
                )
                .into_any_element(),
        ]
    }

    /// The API key reads as a field in both states: a masked key or a
    /// placeholder in an input frame, with a small outline button to edit.
    pub(super) fn render_api_key(
        &mut self,
        disabled: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = cx.palette();
        if self.api_key_editing && !disabled {
            return div()
                .relative()
                // Room for the two buttons, so a long key never runs under them.
                .child(text_field(&self.api_key_input, true, window, cx).pr(px(60.)))
                .child(
                    div()
                        .absolute()
                        .right(px(4.))
                        .top(px(4.))
                        .flex()
                        .gap(px(2.))
                        .child(
                            icon_action(
                                "settings-api-key-save",
                                t("common.save"),
                                "check",
                                p.success,
                                cx,
                            )
                            .on_click(cx.listener(|this, _, _, cx| this.save_api_key(cx))),
                        )
                        .child(
                            icon_action(
                                "settings-api-key-cancel",
                                t("common.cancel"),
                                "x",
                                p.subtle_foreground,
                                cx,
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.api_key_editing = false;
                                cx.notify();
                            })),
                        ),
                )
                .into_any_element();
        }
        let has_key = !self.api_key.is_empty();
        let hover_border = if p.dark {
            p.white.opacity(0.18)
        } else {
            p.border_hover.blend(p.foreground.opacity(0.08))
        };
        let display = div().id("settings-api-key-display");
        let display = if disabled {
            display
        } else {
            display.keyboard_focusable(
                "settings-api-key-display",
                Role::Button,
                if has_key {
                    t("apiKeyInput.editLabel")
                } else {
                    t("apiKeyInput.addLabel")
                },
                cx,
            )
        };
        display
            .group("settings-api-key")
            .w_full()
            .h(px(ROW_CONTROL_HEIGHT))
            .flex()
            .items_center()
            .gap(px(8.))
            .pl(px(12.))
            .pr(px(4.))
            .rounded(px(RADIUS_LG))
            .border_1()
            .border_color(p.input_border)
            .bg(if p.dark {
                p.white.opacity(0.035)
            } else {
                p.input
            })
            .when(!disabled, |d| {
                d.hover(move |s| s.border_color(hover_border))
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, window, cx| this.edit_api_key(window, cx)))
            })
            .child(if has_key {
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(icon("key-round", 12., p.subtle_foreground))
                    .child(
                        div()
                            .truncate()
                            .font_family(FONT_MONO)
                            .text_size(px(TEXT_MONO_BODY))
                            .text_color(p.muted_foreground)
                            .child(mask_key(&self.api_key)),
                    )
                    .into_any_element()
            } else {
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(TEXT_BODY))
                    .text_color(p.subtle_foreground)
                    .child(t("apiKeyInput.placeholder"))
                    .into_any_element()
            })
            .child(
                div()
                    .flex_none()
                    .h(px(24.))
                    .px(px(8.))
                    .flex()
                    .items_center()
                    .rounded(px(RADIUS_MD))
                    .border_1()
                    .border_color(p.border)
                    .text_size(px(TEXT_SMALL))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(p.muted_foreground)
                    .group_hover("settings-api-key", |s| s.text_color(p.foreground))
                    .child(if has_key {
                        t("apiKeyInput.editButton")
                    } else {
                        t("apiKeyInput.addButton")
                    }),
            )
            .into_any_element()
    }
}
