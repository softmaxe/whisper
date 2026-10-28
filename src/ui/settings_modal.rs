//! The Settings modal and the panels it hosts.

use std::time::Duration;

use gpui_kit::component::input::{InputEvent, InputState, TextareaState};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::i18n::{t, t_with};
use crate::settings::{
    ActivationMode, ChineseScriptPreference, MicrophoneSelectionMode, PanelStartPosition, Settings,
    ThemePreference,
};
use crate::text::url::{EndpointIssue, endpoint_check};

use crate::app_state::AppState;
use crate::backend::{self, AutoStartState, MicDeviceInfo, ModelOption};

use super::components::{
    Button, ButtonSize, ButtonVariant, Toggle, icon, panel_row, section_header, settings_field,
    settings_field_tagged, settings_panel, settings_row,
};
use super::hotkey_input::HotkeyListInput;
use super::overlays::{DialogRequest, Toast, show_dialog, show_toast};
use super::select::{Select, SelectEvent, SelectOption, SelectStyle};
use super::text_input::{FieldKind, text_field, textarea_field};
use super::theme::{
    DUR_BASE, DUR_SNAP, FONT_MONO, LINE_SMALL, Palette, PaletteExt, TEXT_BODY, TEXT_CAPTION,
    TEXT_SMALL, ease_out, ease_snap,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsSection {
    General,
    Hotkeys,
    SpeechToText,
    Llms,
    PrivacyData,
}

impl SettingsSection {
    /// Resolves the section names other views link to; History and Upload
    /// link to Speech to Text as "transcription" and "uploadTranscription".
    pub fn from_name(name: &str) -> Self {
        match name {
            "hotkeys" => Self::Hotkeys,
            "speechToText" | "transcription" | "uploadTranscription" => Self::SpeechToText,
            "llms" => Self::Llms,
            "privacyData" => Self::PrivacyData,
            _ => Self::General,
        }
    }

    fn key(self) -> &'static str {
        match self {
            Self::General => "general",
            Self::Hotkeys => "hotkeys",
            Self::SpeechToText => "speechToText",
            Self::Llms => "llms",
            Self::PrivacyData => "privacyData",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Self::General => "sliders",
            Self::Hotkeys => "keyboard",
            Self::SpeechToText => "mic",
            Self::Llms => "brain",
            Self::PrivacyData => "shield",
        }
    }

    fn group(self) -> &'static str {
        match self {
            Self::General | Self::Hotkeys => "settings.groups.app",
            Self::SpeechToText | Self::Llms => "settings.groups.aiModels",
            Self::PrivacyData => "settings.groups.system",
        }
    }
}

const SECTIONS: [SettingsSection; 5] = [
    SettingsSection::General,
    SettingsSection::Hotkeys,
    SettingsSection::SpeechToText,
    SettingsSection::Llms,
    SettingsSection::PrivacyData,
];

pub enum SettingsModalEvent {
    Close,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PromptTab {
    View,
    Edit,
    Test,
}

const RETENTION_DAY_OPTIONS: [u32; 6] = [1, 7, 14, 30, 60, 90];

pub struct SettingsModal {
    section: SettingsSection,
    closing: bool,

    stt_url: Entity<InputState>,
    stt_model: Entity<InputState>,

    cleanup_url: Entity<InputState>,
    api_key: String,
    api_key_editing: bool,
    api_key_input: Entity<InputState>,
    models: Vec<ModelOption>,
    models_loading: bool,
    models_error: Option<String>,
    last_loaded_base: Option<String>,
    models_request: Option<Task<()>>,

    prompt_tab: PromptTab,
    prompt_editor: Entity<TextareaState>,
    test_input: Entity<TextareaState>,
    test_result: String,
    test_loading: bool,
    copied_prompt: bool,

    ui_language: Entity<Select>,
    stt_language: Entity<Select>,
    chinese_script: Entity<Select>,
    pill_position: Entity<Select>,
    audio_retention: Entity<Select>,
    transcript_retention: Entity<Select>,
    microphone: Entity<Select>,

    hotkeys: Entity<HotkeyListInput>,

    auto_start: AutoStartState,
    auto_start_loading: bool,
    mic_devices: Vec<MicDeviceInfo>,
    mic_default: Option<String>,
    storage_usage: (usize, u64),
    mic_permission: bool,
    accessibility_permission: bool,

    scroll: ScrollHandle,
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<SettingsModalEvent> for SettingsModal {}

fn settings(cx: &App) -> Settings {
    AppState::settings(cx)
}

fn update_settings(cx: &mut App, f: impl FnOnce(&mut Settings)) {
    AppState::global(cx).update(cx, |s, cx| s.update_settings(cx, f));
}

fn language_options() -> Vec<SelectOption> {
    backend::languages()
        .into_iter()
        .map(|(code, label, flag)| SelectOption::new(code, label).prefix(flag))
        .collect()
}

fn retention_options(include_disabled_key: &str, current: u32) -> Vec<SelectOption> {
    let mut options = vec![SelectOption::new("0", t(include_disabled_key))];
    if current > 0 && !RETENTION_DAY_OPTIONS.contains(&current) {
        options.push(SelectOption::new(
            current.to_string(),
            t_with(
                "settings.privacy.retentionDays",
                &[("count", &current.to_string())],
            ),
        ));
    }
    for days in RETENTION_DAY_OPTIONS {
        options.push(SelectOption::new(
            days.to_string(),
            t_with(
                "settings.privacy.retentionDays",
                &[("count", &days.to_string())],
            ),
        ));
    }
    options
}

fn position_options() -> Vec<SelectOption> {
    vec![
        SelectOption::new(
            "bottom-right",
            t("settings.general.floatingIcon.bottomRight"),
        ),
        SelectOption::new("center", t("settings.general.floatingIcon.center")),
        SelectOption::new("bottom-left", t("settings.general.floatingIcon.bottomLeft")),
    ]
}

fn chinese_script_options() -> Vec<SelectOption> {
    vec![
        SelectOption::new(
            "as-transcribed",
            t("settings.language.chineseScriptAsTranscribed"),
        ),
        SelectOption::new("simplified", t("settings.language.chineseScriptSimplified")),
        SelectOption::new(
            "traditional",
            t("settings.language.chineseScriptTraditional"),
        ),
    ]
}

fn chinese_script_value(pref: ChineseScriptPreference) -> &'static str {
    match pref {
        ChineseScriptPreference::AsTranscribed => "as-transcribed",
        ChineseScriptPreference::Simplified => "simplified",
        ChineseScriptPreference::Traditional => "traditional",
    }
}

fn mask_key(key: &str) -> String {
    let chars: Vec<char> = key.chars().collect();
    if chars.len() <= 8 {
        return "••••••••".into();
    }
    let head: String = chars[..3].iter().collect();
    let tail: String = chars[chars.len() - 4..].iter().collect();
    format!("{head}...{tail}")
}

/// Default addresses of common OpenAI-compatible chat servers.
const CLEANUP_PRESETS: [(&str, &str); 5] = [
    ("Ollama", "http://localhost:11434/v1"),
    ("LM Studio", "http://localhost:1234/v1"),
    ("vLLM", "http://localhost:8000/v1"),
    ("llama.cpp", "http://localhost:8080/v1"),
    ("OpenAI", "https://api.openai.com/v1"),
];

/// An endpoint input with a live "Will call POST …" line and any problem
/// with the address underneath.
fn endpoint_control(field: impl IntoElement, raw: &str, path: &str, p: &Palette) -> AnyElement {
    let mut col = div().flex().flex_col().gap(px(6.)).child(field);
    if let Some((target, issue)) = endpoint_check(raw, path) {
        let line = |text: String, color: Hsla| {
            div()
                .text_size(px(TEXT_SMALL))
                .line_height(px(LINE_SMALL))
                .text_color(color)
                .child(text)
        };
        if issue != Some(EndpointIssue::Invalid) {
            col = col.child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(6.))
                    .text_size(px(TEXT_SMALL))
                    .line_height(px(LINE_SMALL))
                    .text_color(p.muted_foreground.opacity(0.8))
                    .child(t("settings.endpointHint.willCall"))
                    .child(
                        div()
                            .font_family(FONT_MONO)
                            .text_color(p.foreground.opacity(0.85))
                            .child(format!("POST {target}")),
                    ),
            );
        }
        col = match issue {
            Some(EndpointIssue::Invalid) => {
                col.child(line(t("settings.endpointHint.invalid"), p.destructive))
            }
            Some(EndpointIssue::NeedsHttps) => {
                col.child(line(t("settings.endpointHint.needsHttps"), p.destructive))
            }
            Some(EndpointIssue::NoV1) => {
                col.child(line(t("settings.endpointHint.noV1"), p.warning))
            }
            None => col,
        };
    }
    col.into_any_element()
}

impl SettingsModal {
    pub fn new(section: SettingsSection, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let s = settings(cx);
        let mut subscriptions = Vec::new();

        let stt_url = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("http://192.168.1.126:8000/v1")
                .default_value(s.remote_transcription_url.clone())
        });
        subscriptions.push(cx.subscribe(&stt_url, |_, input, e: &InputEvent, cx| {
            if matches!(e, InputEvent::Change) {
                let value = input.read(cx).value().to_string();
                update_settings(cx, |s| s.remote_transcription_url = value);
                cx.notify();
            }
        }));
        let stt_model = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Whisper-Large-v3-Turbo")
                .default_value(s.remote_transcription_model.clone())
        });
        subscriptions.push(cx.subscribe(&stt_model, |_, input, e: &InputEvent, cx| {
            if matches!(e, InputEvent::Change) {
                let value = input.read(cx).value().to_string();
                update_settings(cx, |s| s.remote_transcription_model = value);
            }
        }));

        let cleanup_url = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("http://192.168.1.126:11434/v1")
                .default_value(s.cleanup_remote_url.clone())
        });
        subscriptions.push(cx.subscribe_in(
            &cleanup_url,
            window,
            |this, _, e: &InputEvent, window, cx| match e {
                InputEvent::Blur => this.apply_cleanup_base_on_blur(window, cx),
                InputEvent::PressEnter { .. } => this.apply_cleanup_base(window, cx),
                InputEvent::Change => cx.notify(),
                _ => {}
            },
        ));

        let api_key_input =
            cx.new(|cx| InputState::new(window, cx).placeholder(t("apiKeyInput.placeholder")));
        subscriptions.push(cx.subscribe_in(
            &api_key_input,
            window,
            |this, _, e: &InputEvent, window, cx| {
                match e {
                    InputEvent::PressEnter { .. } => this.save_api_key(cx),
                    InputEvent::Blur
                        // Commit on click-away rather than discarding a pasted key.
                        if this.api_key_editing => {
                            this.save_api_key(cx);
                        }
                    _ => {}
                }
                let _ = window;
            },
        ));

        let default_prompt = backend::default_cleanup_prompt(&s.ui_language);
        let current_prompt = if s.custom_prompt_cleanup.is_empty() {
            default_prompt.clone()
        } else {
            s.custom_prompt_cleanup.clone()
        };
        let prompt_editor = cx.new(|cx| {
            TextareaState::new(window, cx)
                .placeholder(t("promptStudio.edit.placeholder"))
                .default_value(current_prompt)
        });
        let test_input = cx.new(|cx| {
            TextareaState::new(window, cx)
                .placeholder(t("promptStudio.test.inputPlaceholder"))
                .default_value(t("promptStudio.defaultTestInput"))
        });
        subscriptions.push(cx.subscribe(&test_input, |_, _, _: &InputEvent, cx| cx.notify()));

        let ui_language = cx.new(|cx| {
            Select::new(
                vec![
                    SelectOption::new("en", "English").prefix("🇺🇸"),
                    SelectOption::new("zh-CN", "简体中文").prefix("🇨🇳"),
                ],
                Some(s.ui_language.clone().into()),
                SelectStyle::Compact,
                window,
                cx,
            )
            .with_width(px(140.))
            .with_menu_min_width(px(240.))
        });
        subscriptions.push(cx.subscribe(&ui_language, |_, _, e: &SelectEvent, cx| {
            let SelectEvent::Change(value) = e;
            let value = value.to_string();
            update_settings(cx, |s| s.ui_language = value);
        }));

        let stt_language = cx.new(|cx| {
            Select::new(
                language_options(),
                Some(s.preferred_language.clone().into()),
                SelectStyle::Compact,
                window,
                cx,
            )
            .with_width(px(180.))
            .with_menu_min_width(px(240.))
        });
        subscriptions.push(cx.subscribe(&stt_language, |_, _, e: &SelectEvent, cx| {
            let SelectEvent::Change(value) = e;
            let value = value.to_string();
            update_settings(cx, |s| s.preferred_language = value);
        }));

        let chinese_script = cx.new(|cx| {
            Select::new(
                chinese_script_options(),
                Some(chinese_script_value(s.chinese_script_preference).into()),
                SelectStyle::Compact,
                window,
                cx,
            )
            .with_width(px(176.))
        });
        subscriptions.push(cx.subscribe(&chinese_script, |_, _, e: &SelectEvent, cx| {
            let SelectEvent::Change(value) = e;
            let pref = ChineseScriptPreference::parse(value);
            update_settings(cx, |s| s.chinese_script_preference = pref);
        }));

        let pill_position = cx.new(|cx| {
            Select::new(
                position_options(),
                Some(s.panel_start_position.as_str().into()),
                SelectStyle::Compact,
                window,
                cx,
            )
            .with_width(px(140.))
        });
        subscriptions.push(cx.subscribe(&pill_position, |_, _, e: &SelectEvent, cx| {
            let SelectEvent::Change(value) = e;
            let pos = PanelStartPosition::parse(value);
            update_settings(cx, |s| s.panel_start_position = pos);
        }));

        let audio_retention = cx.new(|cx| {
            Select::new(
                retention_options(
                    "settings.privacy.audioRetentionDisabled",
                    s.audio_retention_days,
                ),
                Some(s.audio_retention_days.to_string().into()),
                SelectStyle::Compact,
                window,
                cx,
            )
            .with_width(px(124.))
        });
        subscriptions.push(cx.subscribe(&audio_retention, |_, _, e: &SelectEvent, cx| {
            let SelectEvent::Change(value) = e;
            if let Ok(days) = value.parse::<u32>() {
                update_settings(cx, |s| s.audio_retention_days = days);
            }
        }));

        let transcript_retention = cx.new(|cx| {
            Select::new(
                retention_options(
                    "settings.privacy.transcriptRetentionForever",
                    s.transcript_retention_days,
                ),
                Some(s.transcript_retention_days.to_string().into()),
                SelectStyle::Compact,
                window,
                cx,
            )
            .with_width(px(124.))
        });
        subscriptions.push(
            cx.subscribe(&transcript_retention, |_, _, e: &SelectEvent, cx| {
                let SelectEvent::Change(value) = e;
                if let Ok(days) = value.parse::<u32>() {
                    update_settings(cx, |s| s.transcript_retention_days = days);
                }
            }),
        );

        let microphone = cx.new(|cx| Select::new(Vec::new(), None, SelectStyle::Field, window, cx));
        subscriptions.push(cx.subscribe(&microphone, |this, _, e: &SelectEvent, cx| {
            let SelectEvent::Change(value) = e;
            if value.as_ref() == "__auto__" {
                update_settings(cx, |s| {
                    s.microphone_selection_mode = MicrophoneSelectionMode::Auto
                });
            } else if let Some(device) = this.mic_devices.iter().find(|d| d.id == value.as_ref()) {
                let (id, label) = (device.id.clone(), device.label.clone());
                update_settings(cx, |s| {
                    s.selected_mic_device_id = id;
                    s.selected_mic_device_label = label;
                    s.microphone_selection_mode = MicrophoneSelectionMode::Specific;
                });
            }
            cx.notify();
        }));

        let hotkeys = cx.new(|cx| HotkeyListInput::new(window, cx));

        let state = AppState::global(cx);
        subscriptions.push(cx.observe(&state, |this, _, cx| {
            this.sync_selects(cx);
            cx.notify();
        }));

        let mut this = Self {
            section,
            closing: false,
            stt_url,
            stt_model,
            cleanup_url,
            api_key: backend::cleanup_api_key(),
            api_key_editing: false,
            api_key_input,
            models: Vec::new(),
            models_loading: false,
            models_error: None,
            last_loaded_base: None,
            models_request: None,
            prompt_tab: PromptTab::View,
            prompt_editor,
            test_input,
            test_result: String::new(),
            test_loading: false,
            copied_prompt: false,
            ui_language,
            stt_language,
            chinese_script,
            pill_position,
            audio_retention,
            transcript_retention,
            microphone,
            hotkeys,
            auto_start: backend::auto_start_state(),
            auto_start_loading: false,
            mic_devices: Vec::new(),
            mic_default: None,
            storage_usage: backend::audio_storage_usage(),
            mic_permission: backend::microphone_permission_granted(),
            accessibility_permission: backend::accessibility_granted(),
            scroll: ScrollHandle::new(),
            focus: cx.focus_handle(),
            _subscriptions: subscriptions,
        };
        this.reload_microphones(cx);
        if !s.cleanup_remote_url.trim().is_empty() {
            this.load_models(None, false, cx);
        }
        this
    }

    pub fn set_section(&mut self, section: SettingsSection, cx: &mut Context<Self>) {
        if self.section != section {
            self.section = section;
            self.scroll.set_offset(point(px(0.), px(0.)));
            if section == SettingsSection::PrivacyData {
                self.storage_usage = backend::audio_storage_usage();
                self.mic_permission = backend::microphone_permission_granted();
                self.accessibility_permission = backend::accessibility_granted();
            }
            cx.notify();
        }
    }

    pub fn request_close(&mut self, cx: &mut Context<Self>) {
        if self.closing {
            return;
        }
        self.closing = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(DUR_SNAP))
                .await;
            let _ = this.update(cx, |_, cx| cx.emit(SettingsModalEvent::Close));
        })
        .detach();
    }

    fn sync_selects(&mut self, cx: &mut Context<Self>) {
        let s = settings(cx);
        self.audio_retention.update(cx, |sel, cx| {
            sel.set_options(
                retention_options(
                    "settings.privacy.audioRetentionDisabled",
                    s.audio_retention_days,
                ),
                cx,
            );
            sel.set_selected(Some(s.audio_retention_days.to_string().into()), cx);
        });
        self.transcript_retention.update(cx, |sel, cx| {
            sel.set_options(
                retention_options(
                    "settings.privacy.transcriptRetentionForever",
                    s.transcript_retention_days,
                ),
                cx,
            );
            sel.set_selected(Some(s.transcript_retention_days.to_string().into()), cx);
            sel.set_disabled(!s.data_retention_enabled, cx);
        });
        self.pill_position.update(cx, |sel, cx| {
            sel.set_options(position_options(), cx);
            sel.set_selected(Some(s.panel_start_position.as_str().into()), cx)
        });
        self.chinese_script
            .update(cx, |sel, cx| sel.set_options(chinese_script_options(), cx));
        self.ui_language.update(cx, |sel, cx| {
            sel.set_selected(Some(s.ui_language.clone().into()), cx)
        });
        self.stt_language.update(cx, |sel, cx| {
            sel.set_selected(Some(s.preferred_language.clone().into()), cx)
        });
        self.sync_microphone_select(cx);
    }

    fn reload_microphones(&mut self, cx: &mut Context<Self>) {
        self.mic_devices = backend::list_microphones();
        self.mic_default = backend::system_default_microphone();
        self.sync_microphone_select(cx);
        cx.notify();
    }

    fn sync_microphone_select(&mut self, cx: &mut Context<Self>) {
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
        for device in &self.mic_devices {
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

    fn selected_device(&self, s: &Settings) -> Option<MicDeviceInfo> {
        match s.microphone_selection_mode {
            MicrophoneSelectionMode::BuiltIn => {
                self.mic_devices.iter().find(|d| d.is_built_in).cloned()
            }
            _ => self
                .mic_devices
                .iter()
                .find(|d| d.id == s.selected_mic_device_id)
                .cloned(),
        }
    }

    // ---- Text cleanup server ----------------------------------------------------------------

    fn cleanup_draft(&self, cx: &App) -> String {
        self.cleanup_url.read(cx).value().trim().to_string()
    }

    fn apply_cleanup_base_on_blur(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let draft = self.cleanup_draft(cx);
        if draft.is_empty() {
            return;
        }
        if draft != settings(cx).cleanup_remote_url.trim() {
            self.apply_cleanup_base(window, cx);
        }
    }

    /// Buttons that fill the endpoint with a known server's default address.
    fn cleanup_presets(&self, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        div()
            .flex()
            .flex_wrap()
            .gap(px(6.))
            .children(CLEANUP_PRESETS.iter().map(|&(name, url)| {
                div()
                    .id(name)
                    .h(px(26.))
                    .px(px(10.))
                    .flex()
                    .items_center()
                    .rounded(px(6.))
                    .border_1()
                    .border_color(if p.dark { p.border_hover } else { p.border })
                    .bg(if p.dark { p.surface_2 } else { p.background })
                    .text_size(px(TEXT_SMALL))
                    .text_color(p.foreground.opacity(0.85))
                    .cursor_pointer()
                    .hover(|s| s.bg(p.hover_wash(0.06)))
                    .child(name)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.cleanup_url
                            .update(cx, |input, cx| input.set_value(url, window, cx));
                        this.apply_cleanup_base(window, cx);
                    }))
            }))
            .into_any_element()
    }

    fn apply_cleanup_base(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let draft = self.cleanup_draft(cx);
        let normalized = if draft.is_empty() {
            draft
        } else {
            backend::normalize_base_url(&draft)
        };
        self.cleanup_url.update(cx, |input, cx| {
            input.set_value(normalized.clone(), window, cx)
        });
        let saved = normalized.clone();
        update_settings(cx, |s| s.cleanup_remote_url = saved);
        self.last_loaded_base = None;
        self.load_models(Some(normalized), true, cx);
    }

    fn refresh_models(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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

    fn load_models(&mut self, base_override: Option<String>, force: bool, cx: &mut Context<Self>) {
        let raw = base_override.unwrap_or_else(|| settings(cx).cleanup_remote_url.clone());
        let normalized = backend::normalize_base_url(&raw);
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

    fn save_api_key(&mut self, cx: &mut Context<Self>) {
        let key = self.api_key_input.read(cx).value().trim().to_string();
        self.api_key = key.clone();
        self.api_key_editing = false;
        backend::set_cleanup_api_key(&key);
        cx.notify();
    }

    fn edit_api_key(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.api_key_editing = true;
        let key = self.api_key.clone();
        self.api_key_input.update(cx, |input, cx| {
            input.set_value(key, window, cx);
            input.focus(window, cx);
            input.select_all(window, cx);
        });
        cx.notify();
    }

    // ---- Prompt Studio ----------------------------------------------------------------------

    fn default_prompt(cx: &App) -> String {
        backend::default_cleanup_prompt(&settings(cx).ui_language)
    }

    fn save_prompt(&mut self, cx: &mut Context<Self>) {
        let edited = self.prompt_editor.read(cx).value().to_string();
        let default = Self::default_prompt(cx);
        // Keep the shipped default eligible for future prompt updates.
        let custom = if edited == default {
            String::new()
        } else {
            edited
        };
        update_settings(cx, |s| s.custom_prompt_cleanup = custom);
        show_dialog(
            DialogRequest::alert(
                t("promptStudio.dialogs.saved.title"),
                Some(t("promptStudio.dialogs.saved.description").into()),
            ),
            cx,
        );
    }

    fn reset_prompt(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let default = Self::default_prompt(cx);
        self.prompt_editor
            .update(cx, |input, cx| input.set_value(default, window, cx));
        update_settings(cx, |s| s.custom_prompt_cleanup = String::new());
        show_dialog(
            DialogRequest::alert(
                t("promptStudio.dialogs.reset.title"),
                Some(t("promptStudio.dialogs.reset.description").into()),
            ),
            cx,
        );
    }

    fn copy_prompt(&mut self, text: String, cx: &mut Context<Self>) {
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

    fn run_prompt_test(&mut self, cx: &mut Context<Self>) {
        let input = self.test_input.read(cx).value().to_string();
        if input.trim().is_empty() || self.test_loading {
            return;
        }
        let s = settings(cx);
        self.test_result.clear();
        if !s.use_cleanup_model {
            self.test_result = t("promptStudio.test.disabledReasoning");
            cx.notify();
            return;
        }
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

    // ---- Rendering --------------------------------------------------------------------------

    fn render_nav(&mut self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let p = cx.palette();
        let mut nav = div().flex().flex_col().px(px(8.)).pt(px(12.)).pb(px(8.));
        let mut last_group = "";
        for (index, section) in SECTIONS.into_iter().enumerate() {
            let group = section.group();
            if group != last_group {
                nav = nav.child(
                    div()
                        .when(index > 0, |d| d.mt(px(16.)))
                        .px(px(8.))
                        .pb(px(4.))
                        .text_size(px(TEXT_CAPTION))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(p.muted_foreground.opacity(if p.dark { 0.65 } else { 0.7 }))
                        .child(t(group)),
                );
                last_group = group;
            }
            let active = self.section == section;
            nav = nav.child(
                div()
                    .id(SharedString::from(format!(
                        "settings-nav-{}",
                        section.key()
                    )))
                    .mt(px(1.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .px(px(8.))
                    .py(px(6.))
                    .rounded(px(6.))
                    .text_size(px(TEXT_BODY))
                    .cursor_pointer()
                    .when(active, |d| {
                        d.bg(if p.dark { p.surface_raised } else { p.muted })
                            .text_color(p.foreground)
                            .font_weight(FontWeight::MEDIUM)
                    })
                    .when(!active, |d| {
                        d.text_color(if p.dark {
                            p.foreground.opacity(0.75)
                        } else {
                            p.muted_foreground
                        })
                        .hover(|s| {
                            s.text_color(p.foreground).bg(if p.dark {
                                p.surface_2
                            } else {
                                p.muted.opacity(0.5)
                            })
                        })
                    })
                    .child(icon(
                        section.icon(),
                        16.,
                        if active {
                            p.primary
                        } else if p.dark {
                            p.foreground.opacity(0.55)
                        } else {
                            p.muted_foreground.opacity(0.7)
                        },
                    ))
                    .child(
                        div()
                            .flex_1()
                            .truncate()
                            .child(t(&format!("settings.sections.{}.label", section.key()))),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.set_section(section, cx))),
            );
        }
        nav
    }

    fn render_general(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let p = cx.palette();
        let s = settings(cx);

        let theme_switch = {
            let mut track = div()
                .flex()
                .items_center()
                .gap(px(1.))
                .p(px(2.))
                .rounded(px(6.))
                .bg(if p.dark {
                    p.surface_2
                } else {
                    p.muted.opacity(0.6)
                });
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
                    ThemePreference::Auto,
                    "monitor",
                    "settings.general.appearance.auto",
                ),
            ] {
                let selected = s.theme == value;
                track = track.child(
                    div()
                        .id(SharedString::from(format!("theme-{key}")))
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .px(px(10.))
                        .py(px(4.))
                        .rounded(px(5.))
                        .text_size(px(TEXT_SMALL))
                        .font_weight(FontWeight::MEDIUM)
                        .cursor_pointer()
                        .when(selected, |d| {
                            d.bg(if p.dark {
                                p.surface_raised
                            } else {
                                p.background
                            })
                            .text_color(p.foreground)
                            .shadow_sm()
                        })
                        .when(!selected, |d| {
                            d.text_color(p.muted_foreground)
                                .hover(|s| s.text_color(p.foreground))
                        })
                        .child(icon(
                            icon_name,
                            12.,
                            if selected {
                                p.primary
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

        let toggle = |id: &'static str, checked: bool, f: fn(&mut Settings, bool)| {
            Toggle::new(id, checked)
                .on_change(move |value, _, cx| update_settings(cx, |s| f(s, value)))
        };

        let auto_start = self.auto_start;
        let mut startup_rows = vec![
            settings_row(
                t("settings.general.startup.launchAtLogin"),
                Some(t("settings.general.startup.launchAtLoginDescription").into()),
                Toggle::new("launch-at-login", auto_start.enabled)
                    .disabled(self.auto_start_loading)
                    .on_change({
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
                    }),
                &p,
            )
            .into_any_element(),
        ];
        if auto_start.requires_approval {
            startup_rows.push(
                panel_row()
                    .child(
                        div()
                            .flex()
                            .gap(px(12.))
                            .p(px(16.))
                            .rounded(px(8.))
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
        startup_rows.push(
            settings_row(
                t("settings.general.startup.startMinimized"),
                Some(t("settings.general.startup.startMinimizedDescription").into()),
                toggle("start-minimized", s.start_minimized, |s, v| {
                    s.start_minimized = v
                }),
                &p,
            )
            .into_any_element(),
        );

        let mut language_rows = vec![
            settings_row(
                t("settings.language.uiLabel"),
                Some(t("settings.language.uiDescription").into()),
                self.ui_language.clone(),
                &p,
            )
            .into_any_element(),
            settings_row(
                t("settings.language.transcriptionLabel"),
                Some(t("settings.language.transcriptionDescription").into()),
                self.stt_language.clone(),
                &p,
            )
            .into_any_element(),
        ];
        if s.preferred_language == "auto" {
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

        div()
            .flex()
            .flex_col()
            .gap(px(24.))
            .child(
                div()
                    .child(section_header(
                        t("settings.general.appearance.title"),
                        Some(t("settings.general.appearance.description").into()),
                        &p,
                    ))
                    .child(settings_panel(
                        vec![
                            settings_row(
                                t("settings.general.appearance.theme"),
                                Some(t("settings.general.appearance.themeDescription").into()),
                                theme_switch,
                                &p,
                            )
                            .into_any_element(),
                            settings_row(
                                t("settings.general.appearance.showMenuBarIcon"),
                                Some(
                                    t("settings.general.appearance.showMenuBarIconDescription")
                                        .into(),
                                ),
                                toggle("menu-bar-icon", s.show_menu_bar_icon, |s, v| {
                                    s.show_menu_bar_icon = v
                                }),
                                &p,
                            )
                            .into_any_element(),
                        ],
                        &p,
                    )),
            )
            .child(
                div()
                    .child(section_header(
                        t("settings.general.soundEffects.title"),
                        None,
                        &p,
                    ))
                    .child(settings_panel(
                        vec![
                            settings_row(
                                t("settings.general.soundEffects.dictationSounds"),
                                Some(
                                    t("settings.general.soundEffects.dictationSoundsDescription")
                                        .into(),
                                ),
                                toggle("audio-cues", s.audio_cues_enabled, |s, v| {
                                    s.audio_cues_enabled = v
                                }),
                                &p,
                            )
                            .into_any_element(),
                            settings_row(
                                t("settings.general.soundEffects.pauseMedia"),
                                Some(
                                    t("settings.general.soundEffects.pauseMediaDescription").into(),
                                ),
                                toggle("pause-media", s.pause_media_on_dictation, |s, v| {
                                    s.pause_media_on_dictation = v
                                }),
                                &p,
                            )
                            .into_any_element(),
                        ],
                        &p,
                    )),
            )
            .child(
                div()
                    .child(section_header(
                        t("settings.general.clipboard.title"),
                        None,
                        &p,
                    ))
                    .child(settings_panel(
                        vec![
                            settings_row(
                                t("settings.general.clipboard.autoPaste"),
                                Some(t("settings.general.clipboard.autoPasteDescription").into()),
                                toggle("auto-paste", s.auto_paste_enabled, |s, v| {
                                    s.auto_paste_enabled = v
                                }),
                                &p,
                            )
                            .into_any_element(),
                            settings_row(
                                t("settings.general.clipboard.keepInClipboard"),
                                Some(
                                    t("settings.general.clipboard.keepInClipboardDescription")
                                        .into(),
                                ),
                                toggle(
                                    "keep-clipboard",
                                    s.keep_transcription_in_clipboard,
                                    |s, v| s.keep_transcription_in_clipboard = v,
                                ),
                                &p,
                            )
                            .into_any_element(),
                        ],
                        &p,
                    )),
            )
            .child(
                div()
                    .child(section_header(
                        t("settings.general.floatingIcon.title"),
                        Some(t("settings.general.floatingIcon.description").into()),
                        &p,
                    ))
                    .child(settings_panel(
                        vec![
                            settings_row(
                                t("settings.general.floatingIcon.startPosition"),
                                Some(
                                    t("settings.general.floatingIcon.startPositionDescription")
                                        .into(),
                                ),
                                self.pill_position.clone(),
                                &p,
                            )
                            .into_any_element(),
                        ],
                        &p,
                    )),
            )
            .child(
                div()
                    .child(section_header(
                        t("settings.language.sectionTitle"),
                        Some(t("settings.language.sectionDescription").into()),
                        &p,
                    ))
                    .child(settings_panel(language_rows, &p)),
            )
            .child(
                div()
                    .child(section_header(
                        t("settings.general.startup.title"),
                        Some(t("settings.general.startup.description").into()),
                        &p,
                    ))
                    .child(settings_panel(startup_rows, &p)),
            )
            .child(
                div()
                    .child(section_header(
                        t("settings.general.microphone.title"),
                        Some(t("settings.general.microphone.description").into()),
                        &p,
                    ))
                    .child(settings_panel(
                        vec![
                            panel_row()
                                .child(self.render_microphone(window, cx))
                                .into_any_element(),
                        ],
                        &p,
                    )),
            )
            .child(
                div()
                    .child(section_header(
                        t("settings.dictionary.autoLearnTitle"),
                        None,
                        &p,
                    ))
                    .child(settings_panel(
                        vec![
                            settings_row(
                                t("settings.dictionary.autoLearnTitle"),
                                Some(t("settings.dictionary.autoLearnDescription").into()),
                                toggle("auto-learn", s.auto_learn_corrections, |s, v| {
                                    s.auto_learn_corrections = v
                                }),
                                &p,
                            )
                            .into_any_element(),
                        ],
                        &p,
                    )),
            )
            .into_any_element()
    }

    fn render_microphone(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let p = cx.palette();
        let s = settings(cx);
        let is_auto = s.microphone_selection_mode == MicrophoneSelectionMode::Auto;
        let built_in = self.mic_devices.iter().find(|d| d.is_built_in).cloned();
        let resolved_label = if is_auto {
            let lid_closed = backend::lid_closed().unwrap_or(false);
            if !lid_closed {
                built_in.as_ref().map(|d| d.label.clone())
            } else {
                None
            }
            .or_else(|| self.mic_default.clone())
            .unwrap_or_else(|| t("microphoneSettings.systemDefault"))
        } else {
            String::new()
        };
        let show_success = is_auto
            || (s.microphone_selection_mode == MicrophoneSelectionMode::BuiltIn
                && built_in.is_some());

        div()
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(12.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                div()
                                    .text_size(px(TEXT_BODY))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(p.foreground)
                                    .child(t("microphoneSettings.inputDevice")),
                            )
                            .child(
                                div()
                                    .id("refresh-mics")
                                    .size(px(28.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded(px(4.))
                                    .cursor_pointer()
                                    .hover(|s| {
                                        s.bg(if p.dark {
                                            p.white.opacity(0.08)
                                        } else {
                                            p.muted
                                        })
                                    })
                                    .child(icon("refresh-cw", 14., p.foreground))
                                    .on_click(
                                        cx.listener(|this, _, _, cx| this.reload_microphones(cx)),
                                    ),
                            ),
                    )
                    .child(self.microphone.clone())
                    .child(
                        div()
                            .text_size(px(TEXT_SMALL))
                            .text_color(p.muted_foreground)
                            .child(t("microphoneSettings.auto.description")),
                    )
                    .when(!is_auto, |d| {
                        d.child(
                            div()
                                .text_size(px(TEXT_SMALL))
                                .text_color(p.muted_foreground)
                                .child(t("microphoneSettings.helpText")),
                        )
                    }),
            )
            .when(show_success, |d| {
                let label = if is_auto {
                    t_with(
                        "microphoneSettings.auto.preferredDevice",
                        &[("device", &resolved_label)],
                    )
                } else {
                    t_with(
                        "microphoneSettings.using",
                        &[("device", &built_in.map(|b| b.label).unwrap_or_default())],
                    )
                };
                d.child(
                    div()
                        .p(px(12.))
                        .rounded(px(8.))
                        .border_1()
                        .border_color(p.success.opacity(0.3))
                        .bg(p.success.opacity(if p.dark { 0.2 } else { 0.1 }))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(icon("mic", 16., p.success))
                        .child(
                            div()
                                .text_size(px(TEXT_BODY))
                                .text_color(p.success)
                                .child(label),
                        ),
                )
            })
    }

    fn render_hotkeys(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let p = cx.palette();
        let s = settings(cx);
        let mode = s.activation_mode;
        let mode_info = backend::hotkey_mode_info(&s.effective_dictation_key());
        let selector = activation_mode_cards(mode, mode_info.push_disabled_reason.clone(), &p);
        div()
            .flex()
            .flex_col()
            .gap(px(24.))
            .child(
                div()
                    .child(section_header(
                        t("settings.general.hotkey.title"),
                        Some(t("settings.general.hotkey.description").into()),
                        &p,
                    ))
                    .child(settings_panel(
                        vec![
                            panel_row().child(self.hotkeys.clone()).into_any_element(),
                            panel_row()
                                .child(
                                    div()
                                        .text_size(px(TEXT_BODY))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(p.foreground)
                                        .child(t("settings.general.hotkey.activationMode")),
                                )
                                .child(div().mt(px(10.)).child(selector))
                                .into_any_element(),
                        ],
                        &p,
                    )),
            )
            .into_any_element()
    }

    fn render_speech_to_text(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let p = cx.palette();
        div()
            .child(section_header(
                t("settings.speechToText.title"),
                Some(t("settings.speechToText.sharedDescription").into()),
                &p,
            ))
            .child(settings_panel(
                vec![
                    settings_field_tagged(
                        t("settings.selfHosted.serverUrl"),
                        "OpenAI · /audio/transcriptions",
                        Some(t("settings.speechToText.serverUrlHelp").into_any_element()),
                        endpoint_control(
                            text_field(&self.stt_url, FieldKind::Line, true, window, cx),
                            self.stt_url.read(cx).value().as_ref(),
                            "/audio/transcriptions",
                            &p,
                        ),
                        &p,
                    )
                    .into_any_element(),
                    settings_field(
                        t("common.model"),
                        None,
                        None,
                        text_field(&self.stt_model, FieldKind::Line, true, window, cx),
                        &p,
                    )
                    .into_any_element(),
                ],
                &p,
            ))
            .into_any_element()
    }

    fn render_llms(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let p = cx.palette();
        let s = settings(cx);
        let mut col = div().flex().flex_col().gap(px(24.)).child(
            div()
                .child(section_header(t("settings.llms.title"), None, &p))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(24.))
                        .child(settings_panel(
                            vec![
                                settings_row(
                                    t("settings.aiModels.enableTextCleanup"),
                                    Some(
                                        t("settings.aiModels.enableTextCleanupDescription").into(),
                                    ),
                                    Toggle::new("use-cleanup", s.use_cleanup_model).on_change(
                                        |v, _, cx| update_settings(cx, |s| s.use_cleanup_model = v),
                                    ),
                                    &p,
                                )
                                .into_any_element(),
                            ],
                            &p,
                        ))
                        .when(s.use_cleanup_model, |d| {
                            d.child(self.render_cleanup_server(window, cx))
                        }),
                ),
        );
        col = col.child(
            div()
                .border_t_1()
                .border_color(p.border.opacity(0.7))
                .pt(px(24.))
                .child(section_header(t("settings.prompts.title"), None, &p))
                .child(self.render_prompt_studio(window, cx)),
        );
        col.into_any_element()
    }

    fn render_cleanup_server(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let p = cx.palette();
        let s = settings(cx);
        let draft = self.cleanup_draft(cx);
        let is_dirty = draft != s.cleanup_remote_url.trim();
        let saved_base = backend::normalize_base_url(&s.cleanup_remote_url);
        let has_base = !saved_base.is_empty();
        let query_base = if has_base {
            saved_base.clone()
        } else {
            "http://192.168.1.126:11434/v1".to_string()
        };
        let query_url = backend::build_api_url(&query_base, "/models").unwrap_or_default();

        let api_key_control = if self.api_key_editing {
            div()
                .relative()
                .child(text_field(
                    &self.api_key_input,
                    FieldKind::Line,
                    true,
                    window,
                    cx,
                ))
                .child(
                    div()
                        .absolute()
                        .right(px(4.))
                        .top(px(4.))
                        .flex()
                        .gap(px(2.))
                        .child(
                            div()
                                .id("api-key-save")
                                .size(px(24.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded(px(4.))
                                .cursor_pointer()
                                .hover(|s| s.bg(p.success.opacity(0.1)))
                                .child(icon("check", 14., p.success))
                                .on_click(cx.listener(|this, _, _, cx| this.save_api_key(cx))),
                        )
                        .child(
                            div()
                                .id("api-key-cancel")
                                .size(px(24.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded(px(4.))
                                .cursor_pointer()
                                .hover(|s| s.bg(p.muted.opacity(0.5)))
                                .child(icon("x", 14., p.muted_foreground.opacity(0.7)))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.api_key_editing = false;
                                    cx.notify();
                                })),
                        ),
                )
                .into_any_element()
        } else {
            let has_key = !self.api_key.is_empty();
            div()
                .id("api-key-display")
                .w_full()
                .h(px(32.))
                .flex()
                .items_center()
                .px(px(12.))
                .rounded(px(4.))
                .border_1()
                .when(has_key, |d| {
                    d.border_color(if p.dark {
                        p.border_subtle.opacity(0.6)
                    } else {
                        p.border.opacity(0.7)
                    })
                    .bg(if p.dark { p.surface_1 } else { p.input })
                })
                .when(!has_key, |d| {
                    d.border_dashed()
                        .border_color(p.border.opacity(0.7))
                        .hover(|s| s.bg(p.muted.opacity(0.3)))
                })
                .cursor_pointer()
                .child(if has_key {
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .font_family(FONT_MONO)
                        .text_size(px(TEXT_SMALL))
                        .text_color(p.foreground.opacity(0.7))
                        .child(icon("key-round", 12., p.muted_foreground.opacity(0.7)))
                        .child(mask_key(&self.api_key))
                        .into_any_element()
                } else {
                    div()
                        .text_size(px(TEXT_SMALL))
                        .text_color(p.muted_foreground.opacity(0.7))
                        .child(t("apiKeyInput.placeholder"))
                        .into_any_element()
                })
                .child(div().flex_1())
                .child(
                    div()
                        .text_size(px(TEXT_SMALL))
                        .text_color(p.muted_foreground.opacity(0.7))
                        .child(if has_key {
                            t("apiKeyInput.editButton")
                        } else {
                            t("apiKeyInput.addButton")
                        }),
                )
                .on_click(cx.listener(|this, _, window, cx| this.edit_api_key(window, cx)))
                .into_any_element()
        };

        let refresh_label = if self.models_loading {
            t("common.loading")
        } else if is_dirty {
            t("reasoning.custom.applyAndRefresh")
        } else {
            t("common.refresh")
        };
        let mut status: Vec<AnyElement> = Vec::new();
        let small = |text: String, color: Hsla| {
            div()
                .text_size(px(TEXT_SMALL))
                .text_color(color)
                .child(text)
                .into_any_element()
        };
        if is_dirty {
            status.push(small(t("reasoning.custom.modelsReloadHint"), p.primary));
        }
        if !has_base {
            status.push(small(t("reasoning.custom.enterEndpoint"), p.warning));
        } else {
            if self.models_loading {
                status.push(small(t("reasoning.custom.fetchingModels"), p.primary));
            }
            if let Some(error) = &self.models_error {
                status.push(small(error.clone(), p.destructive));
            }
            if !self.models_loading && self.models_error.is_none() && self.models.is_empty() {
                status.push(small(t("reasoning.custom.noModels"), p.warning));
            }
            if !self.models_loading && !self.models.is_empty() && s.cleanup_model.is_empty() {
                status.push(small(t("reasoning.custom.selectModelHint"), p.warning));
            }
        }
        let models = if is_dirty {
            Vec::new()
        } else {
            self.models.clone()
        };
        let model_list = model_card_list(&models, &s.cleanup_model, &p);

        div()
            .child(section_header(
                t("settings.llms.server.title"),
                Some(t("settings.llms.server.description").into()),
                &p,
            ))
            .child(settings_panel(
                vec![
                    settings_field_tagged(
                        t("reasoning.custom.endpointTitle"),
                        "OpenAI · /chat/completions",
                        Some(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(10.))
                                .child(t("reasoning.selfHosted.endpointHelp"))
                                .child(self.cleanup_presets(&p, cx))
                                .into_any_element(),
                        ),
                        endpoint_control(
                            text_field(&self.cleanup_url, FieldKind::Line, true, window, cx),
                            &draft,
                            "/chat/completions",
                            &p,
                        ),
                        &p,
                    )
                    .into_any_element(),
                    settings_field(
                        t("reasoning.custom.apiKeyOptional"),
                        Some(small(
                            t("reasoning.custom.apiKeyHelp"),
                            p.muted_foreground.opacity(0.8),
                        )),
                        None,
                        api_key_control,
                        &p,
                    )
                    .into_any_element(),
                    settings_field(
                        t("common.model"),
                        Some(
                            div()
                                .flex()
                                .flex_wrap()
                                .gap(px(4.))
                                .text_size(px(TEXT_SMALL))
                                .text_color(p.muted_foreground.opacity(0.8))
                                .child(t("reasoning.custom.queryPrefix"))
                                .child(div().font_family(FONT_MONO).child(query_url))
                                .child(t("reasoning.custom.querySuffix"))
                                .into_any_element(),
                        ),
                        Some(
                            Button::new("refresh-models")
                                .label(refresh_label)
                                .variant(ButtonVariant::Outline)
                                .size(ButtonSize::Sm)
                                .disabled(self.models_loading || (draft.is_empty() && !has_base))
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.refresh_models(window, cx)
                                }))
                                .into_any_element(),
                        ),
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(8.))
                            .children(status)
                            .child(model_list),
                        &p,
                    )
                    .into_any_element(),
                    settings_row(
                        t("reasoning.disableThinking.label"),
                        Some(t("reasoning.disableThinking.help").into()),
                        Toggle::new("disable-thinking", s.cleanup_disable_thinking).on_change(
                            |v, _, cx| update_settings(cx, |s| s.cleanup_disable_thinking = v),
                        ),
                        &p,
                    )
                    .into_any_element(),
                ],
                &p,
            ))
    }

    fn render_prompt_studio(
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

        let mut tabs = div().flex().border_b_1().border_color(if p.dark {
            p.border_subtle
        } else {
            p.border.opacity(0.7)
        });
        for (value, icon_name, key) in [
            (PromptTab::View, "eye", "promptStudio.tabs.view"),
            (PromptTab::Edit, "edit3", "promptStudio.tabs.customize"),
            (PromptTab::Test, "test-tube", "promptStudio.tabs.test"),
        ] {
            let active = tab == value;
            tabs = tabs.child(
                div()
                    .id(SharedString::from(format!("prompt-tab-{key}")))
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .gap(px(8.))
                    .px(px(16.))
                    .py(px(12.))
                    .text_size(px(TEXT_SMALL))
                    .font_weight(FontWeight::MEDIUM)
                    .border_b_2()
                    .cursor_pointer()
                    .when(active, |d| {
                        d.border_color(p.primary)
                            .text_color(p.foreground)
                            .bg(p.primary.opacity(if p.dark { 0.03 } else { 0.05 }))
                    })
                    .when(!active, |d| {
                        d.border_color(transparent_black())
                            .text_color(p.muted_foreground)
                            .hover(|s| s.text_color(p.foreground).bg(p.hover_wash(0.02)))
                    })
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

        let body = match tab {
            PromptTab::View => {
                let copied = self.copied_prompt;
                let text = current_prompt.replace("{{agentName}}", "Whisper");
                let copy_text = current_prompt.clone();
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
                                            .text_color(p.muted_foreground.opacity(0.7))
                                            .child(
                                                if is_custom {
                                                    t("promptStudio.view.customPrompt")
                                                } else {
                                                    t("promptStudio.view.defaultPrompt")
                                                }
                                                .to_uppercase(),
                                            ),
                                    )
                                    .when(is_custom, |d| {
                                        d.child(
                                            div()
                                                .px(px(6.))
                                                .rounded_full()
                                                .bg(p.primary.opacity(0.1))
                                                .text_size(px(TEXT_CAPTION))
                                                .font_weight(FontWeight::MEDIUM)
                                                .text_color(p.primary)
                                                .child(
                                                    t("promptStudio.view.modified").to_uppercase(),
                                                ),
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
                                    .h(28.)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.copy_prompt(copy_text.clone(), cx)
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .id("prompt-view")
                            .max_h(px(320.))
                            .overflow_y_scroll()
                            .p(px(16.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(p.border.opacity(0.7))
                            .bg(if p.dark {
                                p.surface_raised.opacity(0.3)
                            } else {
                                p.muted.opacity(0.3)
                            })
                            .font_family(FONT_MONO)
                            .text_size(px(TEXT_SMALL))
                            .line_height(px(LINE_SMALL))
                            .text_color(p.muted_foreground)
                            .child(text),
                    )
                    .into_any_element()
            }
            PromptTab::Edit => {
                div()
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
                                    .on_click(cx.listener(|this, _, _, cx| this.save_prompt(cx))),
                            )
                            .child(
                                Button::new("reset-prompt")
                                    .icon("rotate-ccw", 14.)
                                    .label(t("promptStudio.common.reset"))
                                    .variant(ButtonVariant::Outline)
                                    .size(ButtonSize::Sm)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.reset_prompt(window, cx)
                                    })),
                            ),
                    )
                    .into_any_element()
            }
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
                                    .rounded(px(8.))
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
                                            div()
                                                .id("copy-test-output")
                                                .h(px(24.))
                                                .w(px(28.))
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .rounded(px(4.))
                                                .cursor_pointer()
                                                .hover(|s| s.bg(p.muted))
                                                .child(icon("copy", 12., p.muted_foreground))
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
                                        .rounded(px(8.))
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
            .rounded(px(12.))
            .border_1()
            .border_color(if p.dark {
                p.border_subtle
            } else {
                p.border.opacity(0.7)
            })
            .bg(if p.dark { p.surface_2 } else { p.card })
            .overflow_hidden()
            .child(tabs)
            .child(body)
    }

    fn render_privacy(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let p = cx.palette();
        let s = settings(cx);
        let (files, bytes) = self.storage_usage;
        let usage = if files > 0 {
            t_with(
                "settings.privacy.audioStorageFiles",
                &[
                    ("count", &files.to_string()),
                    ("size", &backend::format_bytes(bytes)),
                ],
            )
        } else {
            t("settings.privacy.audioStorageEmpty")
        };
        let block = |content: Div| {
            div()
                .border_t_1()
                .border_color(p.border.opacity(0.7))
                .pt(px(24.))
                .child(content)
        };

        div()
            .flex()
            .flex_col()
            .gap(px(24.))
            .child(block(
                div()
                    .child(section_header(
                        t("settings.privacy.audioRetention"),
                        Some(t("settings.privacy.audioRetentionDescription").into()),
                        &p,
                    ))
                    .child(settings_panel(
                        vec![
                            settings_row(
                                t("settings.privacy.audioRetention"),
                                Some(t("settings.privacy.audioRetentionDescription").into()),
                                self.audio_retention.clone(),
                                &p,
                            )
                            .into_any_element(),
                            settings_row(
                                t("settings.privacy.audioStorageUsage"),
                                Some(usage.into()),
                                Button::new("clear-audio")
                                    .label(t("settings.privacy.clearAllAudio"))
                                    .variant(ButtonVariant::Outline)
                                    .size(ButtonSize::Sm)
                                    .h(28.)
                                    .disabled(files == 0)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        AppState::global(cx).update(cx, |s, cx| s.delete_all_audio(cx));
                                        this.storage_usage = (0, 0);
                                        show_toast(Toast::new(t("settings.privacy.clearAllAudio")), cx);
                                        cx.notify();
                                    })),
                                &p,
                            )
                            .into_any_element(),
                        ],
                        &p,
                    )),
            ))
            .child(block(div().child(settings_panel(
                vec![
                    settings_row(
                        t("settings.privacy.dataRetention"),
                        Some(t("settings.privacy.dataRetentionDescription").into()),
                        Toggle::new("data-retention", s.data_retention_enabled)
                            .on_change(|v, _, cx| update_settings(cx, |s| s.data_retention_enabled = v)),
                        &p,
                    )
                    .into_any_element(),
                    settings_row(
                        t("settings.privacy.transcriptRetention"),
                        Some(t("settings.privacy.transcriptRetentionDescription").into()),
                        self.transcript_retention.clone(),
                        &p,
                    )
                    .into_any_element(),
                ],
                &p,
            ))))
            .child(block(
                div()
                    .child(section_header(
                        t("settings.permissions.title"),
                        Some(t("settings.permissions.description").into()),
                        &p,
                    ))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(12.))
                            .child(permission_card(
                                "perm-mic",
                                "mic",
                                t("settings.permissions.microphoneTitle"),
                                t("settings.permissions.microphoneDescription"),
                                self.mic_permission,
                                cx.listener(|this, _, _, cx| {
                                    backend::request_microphone_permission();
                                    this.mic_permission = backend::microphone_permission_granted();
                                    cx.notify();
                                }),
                                &p,
                            ))
                            .child(permission_card(
                                "perm-ax",
                                "shield",
                                t("settings.permissions.accessibilityTitle"),
                                t("settings.permissions.accessibilityDescription"),
                                self.accessibility_permission,
                                cx.listener(|this, _, _, cx| {
                                    backend::request_accessibility_permission();
                                    this.accessibility_permission = backend::accessibility_granted();
                                    cx.notify();
                                }),
                                &p,
                            )),
                    )
                    .child(
                        div()
                            .mt(px(20.))
                            .child(
                                div()
                                    .mb(px(12.))
                                    .text_size(px(TEXT_SMALL))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(p.foreground)
                                    .child(t("settings.permissions.troubleshootingTitle")),
                            )
                            .child(settings_panel(
                                vec![
                                    settings_row(
                                        t("settings.permissions.resetAccessibility.label"),
                                        Some(t("settings.permissions.resetAccessibility.rowDescription").into()),
                                        Button::new("troubleshoot-ax")
                                            .label(t("settings.permissions.troubleshoot"))
                                            .variant(ButtonVariant::Ghost)
                                            .size(ButtonSize::Sm)
                                            .on_click(|_, _, cx| {
                                                show_dialog(
                                                    DialogRequest::confirm(
                                                        t("settings.permissions.resetAccessibility.title"),
                                                        Some(
                                                            t("settings.permissions.resetAccessibility.description")
                                                                .into(),
                                                        ),
                                                        false,
                                                        |_, _| backend::request_accessibility_permission(),
                                                    ),
                                                    cx,
                                                )
                                            }),
                                        &p,
                                    )
                                    .into_any_element(),
                                ],
                                &p,
                            )),
                    ),
            ))
            .into_any_element()
    }
}

/// Two side-by-side cards, one per activation mode, each explaining itself.
fn activation_mode_cards(
    mode: ActivationMode,
    push_disabled: Option<String>,
    p: &Palette,
) -> impl IntoElement {
    let p = *p;
    let push_blocked = push_disabled.is_some();
    let options = [
        (
            ActivationMode::Tap,
            "mouse-pointer-click",
            "common.tap",
            t("settings.general.hotkey.activationModeTapDescription"),
        ),
        (
            ActivationMode::Push,
            "mic-vocal",
            "common.hold",
            push_disabled
                .unwrap_or_else(|| t("settings.general.hotkey.activationModeHoldDescription")),
        ),
    ];
    div().flex().gap(px(8.)).children(options.into_iter().map(
        move |(value, icon_name, key, description)| {
            let selected = mode == value;
            let disabled = value == ActivationMode::Push && push_blocked;
            let title_color = if selected {
                p.foreground
            } else {
                p.muted_foreground
            };
            div()
                .id(SharedString::from(format!("activation-{key}")))
                .flex_1()
                .min_w_0()
                .p(px(12.))
                .rounded(px(8.))
                .border_1()
                .when(selected, |d| {
                    d.border_color(p.foreground.opacity(if p.dark { 0.3 } else { 0.4 }))
                        .bg(if p.dark {
                            p.surface_raised
                        } else {
                            p.surface_3
                        })
                })
                .when(!selected, |d| {
                    d.border_color(if p.dark { p.border_hover } else { p.border })
                        .bg(if p.dark { p.surface_1 } else { p.background })
                })
                .when(disabled, |d| d.opacity(0.6))
                .when(!disabled && !selected, |d| {
                    d.cursor_pointer()
                        .hover(|s| s.bg(p.hover_wash(0.04)))
                        .on_click(move |_, _, cx| {
                            update_settings(cx, |s| s.activation_mode = value)
                        })
                })
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(icon(icon_name, 14., title_color))
                        .child(
                            div()
                                .text_size(px(TEXT_BODY))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(title_color)
                                .child(t(key)),
                        ),
                )
                .child(
                    div()
                        .mt(px(6.))
                        .text_size(px(TEXT_SMALL))
                        .line_height(px(LINE_SMALL))
                        .text_color(p.muted_foreground.opacity(0.8))
                        .child(description),
                )
        },
    ))
}

fn info_line(label: String, value: String, p: &Palette) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(8.))
        .child(
            div()
                .text_size(px(TEXT_SMALL))
                .text_color(p.muted_foreground.opacity(0.7))
                .child(label.to_uppercase()),
        )
        .child(
            div()
                .font_family(FONT_MONO)
                .text_size(px(TEXT_SMALL))
                .font_weight(FontWeight::MEDIUM)
                .text_color(p.foreground)
                .child(value),
        )
}

/// Fixed widths of the tag and owner columns; the model column takes the rest.
const MODEL_TAG_W: f32 = 140.;
const MODEL_OWNER_W: f32 = 88.;

/// Splits a model id like `hf.co/unsloth/Qwen3-GGUF:Q4_K_M` into its source
/// prefix (`hf.co/unsloth/`), name (`Qwen3-GGUF`) and tag (`Q4_K_M`).
fn split_model_id(id: &str) -> (&str, &str, &str) {
    let slash = id.rfind('/').map_or(0, |i| i + 1);
    let (rest, tag) = match id[slash..].rfind(':') {
        Some(i) => (&id[..slash + i], &id[slash + i + 1..]),
        None => (id, ""),
    };
    (&rest[..slash], &rest[slash..], tag)
}

fn model_card_list(models: &[ModelOption], selected: &str, p: &Palette) -> AnyElement {
    if models.is_empty() {
        return div()
            .text_size(px(TEXT_SMALL))
            .text_color(p.muted_foreground)
            .child(t("models.noneAvailable"))
            .into_any_element();
    }
    let p = *p;
    let line = if p.dark {
        p.white.opacity(0.08)
    } else {
        p.border
    };
    let row = || div().flex().items_center().gap(px(10.)).px(px(12.));
    let header = |text: String| {
        div()
            .text_size(px(TEXT_CAPTION))
            .text_color(p.muted_foreground.opacity(0.7))
            .child(text)
    };
    let mut table = div()
        .flex()
        .flex_col()
        .rounded(px(8.))
        .border_1()
        .border_color(line)
        .bg(if p.dark {
            p.white.opacity(0.02)
        } else {
            p.surface_1
        })
        .overflow_hidden()
        .child(
            row()
                .py(px(7.))
                .border_b_1()
                .border_color(line)
                .child(div().w(px(12.)).flex_none())
                .child(header(t("common.model")).flex_1().min_w_0())
                .child(
                    header(t("reasoning.custom.columnTag"))
                        .w(px(MODEL_TAG_W))
                        .flex_none(),
                )
                .child(
                    header(t("reasoning.custom.columnOwner"))
                        .w(px(MODEL_OWNER_W))
                        .flex_none(),
                ),
        );
    for (index, model) in models.iter().enumerate() {
        let is_selected = model.value == selected;
        let value = model.value.clone();
        let (source, name, tag) = split_model_id(&model.label);
        let muted = |text: String| {
            div()
                .flex_none()
                .truncate()
                .text_size(px(TEXT_CAPTION))
                .text_color(p.muted_foreground.opacity(0.7))
                .child(text)
        };
        table = table.child(
            row()
                .id(("model-card", index))
                .py(px(9.))
                .when(index > 0, |d| d.border_t_1().border_color(line))
                .cursor_pointer()
                .when(is_selected, |d| d.bg(p.hover_wash(0.06)))
                .when(!is_selected, |d| d.hover(|s| s.bg(p.hover_wash(0.03))))
                .on_click(move |_, _, cx| {
                    let value = value.clone();
                    update_settings(cx, |s| s.cleanup_model = value)
                })
                .child(
                    div()
                        .size(px(12.))
                        .flex_none()
                        .rounded_full()
                        .when(is_selected, |d| d.border_4().border_color(p.foreground))
                        .when(!is_selected, |d| {
                            d.border_1().border_color(p.muted_foreground.opacity(0.6))
                        }),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(
                            div()
                                .flex()
                                .overflow_hidden()
                                .font_family(FONT_MONO)
                                .text_size(px(TEXT_SMALL))
                                .font_weight(FontWeight::MEDIUM)
                                .when(!source.is_empty(), |d| {
                                    d.child(
                                        div()
                                            .flex_shrink(1.)
                                            .min_w_0()
                                            .truncate()
                                            .text_color(p.muted_foreground.opacity(0.7))
                                            .child(source.to_string()),
                                    )
                                })
                                .child(
                                    div()
                                        .flex_none()
                                        .text_color(p.foreground)
                                        .child(name.to_string()),
                                ),
                        )
                        .when_some(model.description.clone(), |d, desc| {
                            d.child(
                                div()
                                    .mt(px(2.))
                                    .truncate()
                                    .text_size(px(TEXT_CAPTION))
                                    .text_color(p.muted_foreground.opacity(0.7))
                                    .child(desc),
                            )
                        }),
                )
                .child(
                    muted(if tag.is_empty() {
                        "—".to_string()
                    } else {
                        tag.to_string()
                    })
                    .w(px(MODEL_TAG_W))
                    .font_family(FONT_MONO),
                )
                .child(
                    muted(model.owner.clone().unwrap_or_else(|| "—".to_string()))
                        .w(px(MODEL_OWNER_W)),
                ),
        );
    }
    table.into_any_element()
}

fn permission_card(
    id: &'static str,
    icon_name: &'static str,
    title: String,
    description: String,
    granted: bool,
    on_request: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    p: &Palette,
) -> impl IntoElement {
    div()
        .p(px(12.))
        .rounded(px(6.))
        .border_1()
        .when(granted, |d| {
            d.bg(p.success.opacity(0.05))
                .border_color(p.success.opacity(if p.dark { 0.15 } else { 0.2 }))
        })
        .when(!granted, |d| d.bg(p.surface_1).border_color(p.border))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(12.))
                .child(
                    div()
                        .size(px(32.))
                        .flex_none()
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(6.))
                        .bg(if granted {
                            p.success.opacity(if p.dark { 0.15 } else { 0.1 })
                        } else {
                            p.primary.opacity(if p.dark { 0.15 } else { 0.1 })
                        })
                        .child(if granted {
                            icon("check", 16., p.success)
                        } else {
                            icon(icon_name, 16., p.primary)
                        }),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(
                            div()
                                .text_size(px(TEXT_SMALL))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(p.foreground)
                                .child(title),
                        )
                        .child(
                            div()
                                .mt(px(2.))
                                .text_size(px(TEXT_SMALL))
                                .text_color(p.muted_foreground)
                                .child(description),
                        ),
                )
                .when(!granted, |d| {
                    d.child(
                        Button::new(id)
                            .label(t("settings.permissions.grantAccess"))
                            .size(ButtonSize::Sm)
                            .h(28.)
                            .on_click(on_request),
                    )
                }),
        )
}

impl Focusable for SettingsModal {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for SettingsModal {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = cx.palette();
        let nav = self.render_nav(cx);
        let content = match self.section {
            SettingsSection::General => self.render_general(window, cx),
            SettingsSection::Hotkeys => self.render_hotkeys(cx),
            SettingsSection::SpeechToText => self.render_speech_to_text(window, cx),
            SettingsSection::Llms => self.render_llms(window, cx),
            SettingsSection::PrivacyData => self.render_privacy(cx),
        };
        let viewport = window.viewport_size();
        let width = (viewport.width * 0.9).min(px(896.));
        let height = viewport.height * 0.85;
        let compact = width < px(800.);
        let closing = self.closing;

        let panel = div()
            .id("settings-panel")
            .occlude()
            .relative()
            .w(width)
            .h(height)
            .flex()
            .rounded(px(12.))
            .overflow_hidden()
            .border_1()
            .border_color(if p.dark { p.border_subtle } else { p.border })
            .bg(if p.dark { p.surface_1 } else { p.background })
            .shadow(vec![BoxShadow {
                color: hsla(0., 0., 0., if p.dark { 0.5 } else { 0.25 }),
                offset: point(px(0.), px(25.)),
                blur_radius: px(50.),
                spread_radius: px(-12.),
                inset: false,
            }])
            .on_click(|_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .w(px(if compact { 48. } else { 208. }))
                    .flex_none()
                    .h_full()
                    .border_r_1()
                    .border_color(if p.dark {
                        p.border_subtle
                    } else {
                        p.border.opacity(0.7)
                    })
                    .bg(if p.dark { p.surface_0 } else { p.surface_1 })
                    .child(nav),
            )
            .child(
                div()
                    .id("settings-content")
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .bg(if p.dark { p.surface_1 } else { p.background })
                    .child(div().p(px(if compact { 16. } else { 24. })).child(content)),
            )
            .child(
                div()
                    .id("settings-close")
                    .absolute()
                    .top(px(16.))
                    .right(px(16.))
                    .p(px(6.))
                    .rounded(px(6.))
                    .opacity(0.4)
                    .cursor_pointer()
                    .hover(|s| {
                        s.opacity(1.)
                            .bg(if p.dark { p.surface_raised } else { p.muted })
                    })
                    .child(icon("x", 14., p.muted_foreground))
                    .on_click(cx.listener(|this, _, _, cx| this.request_close(cx))),
            )
            .with_animation(
                ("settings-panel-anim", closing as usize),
                Animation::new(Duration::from_millis(if closing {
                    DUR_SNAP
                } else {
                    DUR_BASE
                }))
                .with_easing(if closing { ease_snap } else { ease_out }),
                move |d, t| {
                    let v = if closing { 1. - t } else { t };
                    // zoom-in-98: approximate the scale with a small lift.
                    d.opacity(v).mt(px(6. * (1. - v)))
                },
            );

        div()
            .id("settings-backdrop")
            .track_focus(&self.focus)
            .key_context("SettingsModal")
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(hsla(0., 0., 0., 0.72))
            .occlude()
            .on_click(cx.listener(|this, _, _, cx| this.request_close(cx)))
            .child(panel)
            .with_animation(
                ("settings-backdrop-anim", closing as usize),
                Animation::new(Duration::from_millis(if closing {
                    DUR_SNAP
                } else {
                    DUR_BASE
                }))
                .with_easing(if closing { ease_snap } else { ease_out }),
                move |d, t| d.opacity(if closing { 1. - t } else { t }),
            )
    }
}
