//! The Settings modal and the panels it hosts.

use std::time::Duration;

use std::rc::Rc;

use gpui_kit::base::FocusTrapElement as _;
use gpui_kit::component::input::{InputEvent, InputState, TextareaState};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::i18n::{t, t_with};
use crate::settings::{
    ActivationMode, ChineseScriptPreference, MicrophoneSelectionMode, PanelStartPosition, Settings,
    ThemePreference,
};
use crate::text::hotkeys::{Hotkey, parse_hotkey_list};
use crate::text::url::{EndpointIssue, endpoint_check};

use crate::app_state::AppState;
use crate::backend::{self, AutoStartState};
use crate::native::mic::MicDevice;
use crate::services::cleanup::ModelOption;
use crate::text::prompts::default_cleanup_prompt;
use crate::text::url::normalize_base_url;

use super::components::f32_tx;
use super::components::{
    Button, ButtonSize, ButtonVariant, KeyboardFocusable, NavSurface, Toggle, backdrop,
    dialog_shadow, icon, kbd, keyed_focus_handle, nav_row, page_container, panel_row,
    section_header, segment, segmented_track, settings_field, settings_field_tagged, settings_row,
    settings_section, status_label,
};
use super::control_panel::{CloseOverlay, SIDEBAR_WIDTH, drag_region};
use super::hotkey_input::{HotkeyListInput, hotkey_glyphs};
use super::overlays::{DialogRequest, Overlays, Toast, ToastVariant, show_dialog, show_toast};
use super::select::{Select, SelectEvent, SelectOption};
use super::text_input::{text_field, text_field_disabled, textarea_field};
use super::theme::{
    DUR_BASE, DUR_FAST, DUR_SNAP, FONT_MONO, LINE_SMALL, Palette, PaletteExt, RADIUS_2XL,
    RADIUS_LG, RADIUS_MD, RADIUS_SM, RADIUS_XL, ROW_CONTROL_HEIGHT, TEXT_BODY, TEXT_CAPTION,
    TEXT_MONO_BODY, TEXT_MONO_CAPTION, TEXT_MONO_SMALL, TEXT_SMALL, WORKSPACE_TOP_BAR_HEIGHT,
    ease_out, ease_snap, motion,
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

/// What a server page's Connection row reports.
#[derive(Clone, PartialEq)]
enum ConnectionCheck {
    Idle,
    Checking,
    Connected {
        model_listed: Option<bool>,
    },
    /// The raw error; the row words it for people and keeps it under Details.
    Failed(String),
}

impl ConnectionCheck {
    /// Whether the server listed the model in the field, when both are known.
    fn connected(models: &[String], model: &str) -> Self {
        Self::Connected {
            model_listed: (!model.is_empty() && !models.is_empty())
                .then(|| models.iter().any(|m| m == model)),
        }
    }
}

/// One width for every select in a settings row, so the control column keeps
/// a single left edge down the page.
const SELECT_WIDTH: f32 = 200.;

/// The input device select grows past `SELECT_WIDTH` up to this cap for long
/// device names; longer ones truncate and show in full in the menu.
const MIC_SELECT_WIDTH: f32 = 240.;

const RETENTION_DAY_OPTIONS: [u32; 6] = [1, 7, 14, 30, 60, 90];

pub struct SettingsModal {
    section: SettingsSection,
    closing: bool,

    stt_url: Entity<InputState>,
    stt_model: Entity<InputState>,

    cleanup_url: Entity<InputState>,
    cleanup_model: Entity<InputState>,
    api_key: String,
    api_key_editing: bool,
    api_key_input: Entity<InputState>,
    models: Vec<ModelOption>,
    models_loading: bool,
    models_error: Option<String>,
    models_error_details: bool,
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

    stt_check: ConnectionCheck,
    stt_error_details: bool,
    /// Models the speech server listed on the last successful check.
    stt_models: Vec<String>,
    stt_request: Option<Task<()>>,

    auto_start: AutoStartState,
    auto_start_loading: bool,
    mic_devices: Vec<MicDevice>,
    mic_default: Option<String>,
    storage_usage: (usize, u64),
    mic_permission: bool,
    accessibility_permission: bool,

    scroll: ScrollHandle,
    /// The section whose scroll extent has been measured for the bottom fade.
    measured_section: Option<SettingsSection>,
    focus: FocusHandle,
    /// Where keyboard focus was when the sheet opened; it returns there on
    /// close, like a macOS sheet.
    restore_focus: Option<FocusHandle>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<SettingsModalEvent> for SettingsModal {}

fn settings(cx: &App) -> Settings {
    AppState::settings(cx)
}

fn update_settings(cx: &mut App, f: impl FnOnce(&mut Settings)) {
    AppState::global(cx).update(cx, |s, cx| s.update_settings(cx, f));
}

/// Languages by their own names, which is how speakers look for them; the
/// English name still matches a search. Auto-detect leads with a globe.
fn language_options() -> Vec<SelectOption> {
    crate::text::language::languages()
        .iter()
        .map(|l| {
            if l.code == "auto" {
                SelectOption::new("auto", t("settings.language.autoDetect"))
                    .icon("globe")
                    .keywords(l.label.clone())
            } else {
                SelectOption::new(l.code.clone(), l.native.clone()).keywords(l.label.clone())
            }
        })
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
    format!("{head}…{tail}")
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
                    .text_color(p.subtle_foreground)
                    .child(t("settings.endpointHint.willCall"))
                    .child(
                        div()
                            .font_family(FONT_MONO)
                            .text_size(px(TEXT_MONO_SMALL))
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
        subscriptions.push(cx.subscribe(&stt_url, |this, input, e: &InputEvent, cx| {
            if matches!(e, InputEvent::Change) {
                let value = input.read(cx).value().to_string();
                update_settings(cx, |s| s.remote_transcription_url = value);
                this.reset_stt_check();
                cx.notify();
            }
        }));
        let stt_model = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Whisper-Large-v3-Turbo")
                .default_value(s.remote_transcription_model.clone())
        });
        subscriptions.push(cx.subscribe(&stt_model, |this, input, e: &InputEvent, cx| {
            if matches!(e, InputEvent::Change) {
                let value = input.read(cx).value().to_string();
                this.recheck_model_listed(value.trim());
                update_settings(cx, |s| s.remote_transcription_model = value);
                cx.notify();
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

        let cleanup_model = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("qwen3:8b")
                .default_value(s.cleanup_model.clone())
        });
        subscriptions.push(
            cx.subscribe(&cleanup_model, |_, input, e: &InputEvent, cx| {
                if matches!(e, InputEvent::Change) {
                    let value = input.read(cx).value().trim().to_string();
                    update_settings(cx, |s| s.cleanup_model = value);
                    cx.notify();
                }
            }),
        );

        let api_key_input =
            cx.new(|cx| InputState::new(window, cx).placeholder(t("apiKeyInput.placeholder")));
        subscriptions.push(
            cx.subscribe(&api_key_input, |this, _, e: &InputEvent, cx| match e {
                InputEvent::PressEnter { .. } => this.save_api_key(cx),
                InputEvent::Blur
                    // Commit on click-away rather than discarding a pasted key.
                    if this.api_key_editing => {
                        this.save_api_key(cx);
                    }
                _ => {}
            }),
        );

        let current_prompt = if s.custom_prompt_cleanup.is_empty() {
            default_cleanup_prompt(&s.ui_language).to_string()
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
                    SelectOption::new("en", "English"),
                    SelectOption::new("zh-CN", "简体中文"),
                ],
                Some(s.ui_language.clone().into()),
                window,
                cx,
            )
            .with_width(px(SELECT_WIDTH))
            .with_menu_min_width(px(240.))
            .with_label(t("settings.language.uiLabel"))
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
                window,
                cx,
            )
            .with_width(px(SELECT_WIDTH))
            .with_menu_min_width(px(240.))
            .with_label(t("settings.language.transcriptionLabel"))
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
                window,
                cx,
            )
            .with_width(px(SELECT_WIDTH))
            .with_label(t("settings.language.chineseScriptLabel"))
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
                window,
                cx,
            )
            .with_width(px(SELECT_WIDTH))
            .with_label(t("settings.general.floatingIcon.startPosition"))
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
                window,
                cx,
            )
            .with_width(px(SELECT_WIDTH))
            .with_label(t("settings.privacy.keepAudioFor"))
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
                window,
                cx,
            )
            .with_width(px(SELECT_WIDTH))
            .with_label(t("settings.privacy.transcriptRetention"))
        });
        subscriptions.push(
            cx.subscribe(&transcript_retention, |_, _, e: &SelectEvent, cx| {
                let SelectEvent::Change(value) = e;
                if let Ok(days) = value.parse::<u32>() {
                    update_settings(cx, |s| s.transcript_retention_days = days);
                }
            }),
        );

        let microphone = cx.new(|cx| {
            Select::new(Vec::new(), None, window, cx)
                .with_width_range(px(SELECT_WIDTH), px(MIC_SELECT_WIDTH))
                .with_menu_min_width(px(280.))
                .with_label(t("microphoneSettings.inputDevice"))
        });
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

        let hotkeys = cx.new(HotkeyListInput::new);

        // Keyboard focus moves into the sheet, so Tab starts on its first
        // control and Escape reaches it.
        let focus = cx.focus_handle();
        window.focus(&focus, cx);

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
            cleanup_model,
            api_key: backend::cleanup_api_key(),
            api_key_editing: false,
            api_key_input,
            models: Vec::new(),
            models_loading: false,
            models_error: None,
            models_error_details: false,
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
            stt_check: ConnectionCheck::Idle,
            stt_error_details: false,
            stt_models: Vec::new(),
            stt_request: None,
            auto_start: backend::auto_start_state(),
            auto_start_loading: false,
            mic_devices: Vec::new(),
            mic_default: None,
            storage_usage: backend::audio_storage_usage(),
            mic_permission: backend::microphone_permission_granted(),
            accessibility_permission: backend::accessibility_granted(),
            scroll: ScrollHandle::new(),
            measured_section: None,
            focus,
            restore_focus: window.focused(cx),
            _subscriptions: subscriptions,
        };
        this.reload_microphones(cx);
        this.sync_selects(cx);
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

    /// Closes the sheet and returns keyboard focus to where it was before.
    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(handle) = self.restore_focus.take() {
            window.focus(&handle, cx);
        }
        self.request_close(cx);
    }

    /// Escape peels one layer at a time: an API key edit first, then the
    /// sheet. Dialogs and the command palette above the sheet close first.
    fn on_close_overlay(&mut self, _: &CloseOverlay, window: &mut Window, cx: &mut Context<Self>) {
        let overlays = Overlays::global(cx).read(cx);
        if overlays.has_dialog() || overlays.has_custom() {
            cx.propagate();
            return;
        }
        if self.api_key_editing {
            self.api_key_editing = false;
            cx.notify();
            return;
        }
        self.close(window, cx);
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
            sel.set_disabled(!s.data_retention_enabled, cx);
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

    fn selected_device(&self, s: &Settings) -> Option<MicDevice> {
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

    /// Shortcuts that fill the Server URL with a known server's default
    /// address. They are not a selection: a custom address matches none, and
    /// the one whose address the field holds carries a check.
    fn cleanup_presets(&self, draft: &str, disabled: bool, cx: &mut Context<Self>) -> AnyElement {
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

    /// Puts a listed model in the Model field, as if typed.
    fn pick_cleanup_model(&mut self, value: String, window: &mut Window, cx: &mut Context<Self>) {
        self.cleanup_model
            .update(cx, |input, cx| input.set_value(value.clone(), window, cx));
        update_settings(cx, |s| s.cleanup_model = value);
        cx.notify();
    }

    fn apply_cleanup_base(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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

    // ---- Speech to text server --------------------------------------------------------------

    fn reset_stt_check(&mut self) {
        self.stt_request = None;
        self.stt_check = ConnectionCheck::Idle;
        self.stt_models.clear();
    }

    /// Keeps "model available" true to the field after a check, without
    /// another request, since the server's list is already known.
    fn recheck_model_listed(&mut self, model: &str) {
        if matches!(self.stt_check, ConnectionCheck::Connected { .. }) {
            self.stt_check = ConnectionCheck::connected(&self.stt_models, model);
        }
    }

    /// Asks the server for its model list: a cheap request every
    /// OpenAI-compatible server answers, which proves the address works.
    fn test_stt_connection(&mut self, cx: &mut Context<Self>) {
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
                        this.stt_models = models.into_iter().map(|m| m.value).collect();
                        ConnectionCheck::connected(&this.stt_models, &model)
                    }
                    Err(error) => ConnectionCheck::Failed(error),
                };
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
        default_cleanup_prompt(&settings(cx).ui_language).to_string()
    }

    /// Saving is routine, so it confirms with a toast rather than a dialog
    /// to dismiss.
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
        show_toast(
            Toast::new(t("promptStudio.dialogs.saved.title"))
                .description(t("promptStudio.dialogs.saved.description"))
                .variant(ToastVariant::Success),
            cx,
        );
    }

    /// Resetting discards the custom prompt, so it asks first.
    fn confirm_reset_prompt(&mut self, cx: &mut Context<Self>) {
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

    /// The section list. Over the full window its top band is left to the
    /// traffic lights and moves the window, like the app sidebar's.
    fn render_nav(&mut self, attached: bool, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let p = cx.palette();
        let mut nav = div()
            .flex()
            .flex_col()
            .px(px(8.))
            .pt(px(if attached { 0. } else { 12. }))
            .pb(px(8.))
            .when(attached, |d| {
                d.child(drag_region().h(px(WORKSPACE_TOP_BAR_HEIGHT)).flex_none())
            });
        let mut last_group = "";
        for (index, section) in SECTIONS.into_iter().enumerate() {
            let group = section.group();
            if group != last_group {
                nav = nav.child(
                    div()
                        .when(index > 0, |d| d.mt(px(16.)))
                        .px(px(10.))
                        .pb(px(4.))
                        // A caption-size semibold tier, so group names never
                        // read as one more row description.
                        .text_size(px(TEXT_CAPTION))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(p.subtle_foreground)
                        .child(t(group)),
                );
                last_group = group;
            }
            let active = self.section == section;
            nav = nav.child(
                div().mt(px(2.)).child(
                    nav_row(
                        SharedString::from(format!("settings-nav-{}", section.key())),
                        section.icon(),
                        t(&format!("settings.sections.{}.label", section.key())),
                        active,
                        NavSurface::Settings,
                        cx,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.set_section(section, cx))),
                ),
            );
        }
        nav
    }

    fn render_general(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let p = cx.palette();
        let s = settings(cx);

        let theme_switch = {
            let mut track = segmented_track(&p).items_center().gap(px(2.));
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
                Some(t("settings.general.appearance.description").into()),
                vec![
                    settings_row(
                        t("settings.general.appearance.theme"),
                        Some(t("settings.general.appearance.themeDescription").into()),
                        theme_switch,
                        &p,
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
                Some(t("settings.language.uiSectionDescription").into()),
                vec![self.render_ui_language(cx)],
                &p,
            ))
            .child(settings_section(
                t("settings.general.soundEffects.title"),
                Some(t("settings.general.soundEffects.description").into()),
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
                Some(t("settings.general.clipboard.description").into()),
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
                Some(t("settings.general.startup.description").into()),
                startup_rows,
                &p,
            ))
            .child(settings_section(
                t("settings.dictionary.title"),
                Some(t("settings.dictionary.description").into()),
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
    fn render_ui_language(&mut self, cx: &mut Context<Self>) -> AnyElement {
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
            .max_w(px(super::components::DESCRIPTION_MAX_WIDTH))
            .flex()
            .flex_wrap()
            .gap_x(px(3.5))
            .text_size(px(TEXT_SMALL))
            .line_height(px(LINE_SMALL))
            .text_color(p.subtle_foreground)
            .child(t("settings.language.uiDescription"))
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

    /// The input device row: the select sits inline like every other
    /// select, and what Auto does is a quiet note under the label.
    fn render_microphone(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let p = cx.palette();
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
                                    .max_w(px(super::components::DESCRIPTION_MAX_WIDTH))
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

    /// The row label and device name to report under the picker, when the
    /// selection resolves to a device without the user reading the dropdown.
    fn microphone_status(&self, cx: &App) -> Option<(String, String)> {
        let built_in = self.mic_devices.iter().find(|d| d.is_built_in);
        match settings(cx).microphone_selection_mode {
            MicrophoneSelectionMode::Auto => {
                let lid_closed = backend::lid_closed().unwrap_or(false);
                let device = built_in
                    .filter(|_| !lid_closed)
                    .map(|d| d.label.clone())
                    .or_else(|| self.mic_default.clone())
                    .unwrap_or_else(|| t("microphoneSettings.systemDefault"));
                Some((t("microphoneSettings.currentlyUsing"), device))
            }
            MicrophoneSelectionMode::BuiltIn => {
                built_in.map(|d| (t("microphoneSettings.currentlyUsing"), d.label.clone()))
            }
            MicrophoneSelectionMode::Specific => None,
        }
    }

    /// The Dictation page: everything about the act of dictating, from the
    /// keys that start it to the microphone and the pill it shows.
    fn render_hotkeys(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let p = cx.palette();
        let mut microphone_rows = vec![self.render_microphone(cx)];
        if let Some((label, device)) = self.microphone_status(cx) {
            microphone_rows
                .push(settings_row(label, None, status_label(device, &p), &p).into_any_element());
        }
        let s = settings(cx);
        let mode = s.activation_mode;
        let keys = s.effective_dictation_key();
        let mode_info = backend::hotkey_mode_info(&keys);
        let selector = activation_mode_cards(
            mode,
            TapStyle::of(&parse_hotkey_list(&keys)),
            mode_info.push_disabled_reason.clone(),
            cx,
        );
        div()
            .flex()
            .flex_col()
            .gap(px(24.))
            .child(settings_section(
                t("settings.general.hotkey.title"),
                Some(t("settings.general.hotkey.description").into()),
                vec![self.hotkeys.clone().into_any_element()],
                &p,
            ))
            .child(
                div()
                    .child(section_header(
                        t("settings.general.hotkey.activationMode"),
                        Some(t("settings.general.hotkey.activationModeDescription").into()),
                        &p,
                    ))
                    .child(selector),
            )
            .child(settings_section(
                t("settings.general.microphone.title"),
                Some(t("settings.general.microphone.description").into()),
                microphone_rows,
                &p,
            ))
            .child(settings_section(
                t("settings.general.floatingIcon.title"),
                Some(t("settings.general.floatingIcon.description").into()),
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

    /// The page heading is the breadcrumb; the panel opens with the server,
    /// titled like the Text cleanup server so the two pages read alike.
    fn render_speech_to_text(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
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
            Some(t("settings.speechToText.sharedDescription").into()),
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
                    None,
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
                Some(t("settings.language.transcriptionSectionDescription").into()),
                language_rows,
                &p,
            ))
            .into_any_element()
    }

    /// The models the server listed, as chips that fill the free-text Model
    /// field; the one matching the field carries a check.
    fn stt_model_choices(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = cx.palette();
        let current = self.stt_model.read(cx).value().trim().to_string();
        let mut row = div().flex().flex_wrap().gap(px(6.));
        for (index, model) in self.stt_models.iter().take(STT_MODEL_CHOICES).enumerate() {
            let value = model.clone();
            row = row.child(
                suggestion_chip(
                    ("settings-stt-model", index),
                    model.clone(),
                    model.clone(),
                    *model == current,
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

    fn render_stt_check(&mut self, cx: &mut Context<Self>) -> AnyElement {
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
    fn render_llms(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let p = cx.palette();
        let s = settings(cx);
        let enabled = s.use_cleanup_model;
        let master = settings_section(
            t("settings.llms.cleanup.title"),
            Some(t("settings.llms.cleanup.description").into()),
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
            Some(t("settings.llms.output.description").into()),
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
                settings_section(
                    t("settings.llms.server.title"),
                    Some(t("settings.llms.server.description").into()),
                    server_rows,
                    &p,
                )
                .when(!enabled, |d| d.opacity(0.5)),
            )
            .child(
                div()
                    .child(section_header(
                        t("settings.prompts.title"),
                        Some(t("settings.prompts.description").into()),
                        &p,
                    ))
                    .child(self.render_prompt_studio(window, cx)),
            )
            .child(output.when(!enabled, |d| d.opacity(0.5)))
            .into_any_element()
    }

    /// The cleanup server's Connection state, read off the model fetch that
    /// Test Connection (and applying a new URL) runs.
    fn cleanup_check(&self, is_dirty: bool, cx: &App) -> ConnectionCheck {
        if self.models_loading {
            ConnectionCheck::Checking
        } else if let Some(error) = &self.models_error {
            ConnectionCheck::Failed(error.clone())
        } else if self.last_loaded_base.is_some() && !is_dirty {
            let listed: Vec<String> = self.models.iter().map(|m| m.value.clone()).collect();
            ConnectionCheck::connected(&listed, settings(cx).cleanup_model.trim())
        } else {
            ConnectionCheck::Idle
        }
    }

    fn render_cleanup_server(
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
        let models = if is_dirty || disabled {
            Vec::new()
        } else {
            self.models.clone()
        };
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
                None,
                self.render_api_key(disabled, window, cx),
                &p,
            )
            .into_any_element(),
            settings_field(
                t("common.model"),
                Some(t("reasoning.custom.modelHelp").into_any_element()),
                None,
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(text_field_disabled(
                        &self.cleanup_model,
                        true,
                        disabled,
                        window,
                        cx,
                    ))
                    .when(!models.is_empty(), |d| {
                        d.child(model_card_list(
                            &models,
                            &model,
                            cx.entity().downgrade(),
                            &p,
                            cx,
                        ))
                    }),
                &p,
            )
            .into_any_element(),
            connection_row(
                "settings-cleanup-test",
                status,
                can_test,
                cx.listener(|this, _, window, cx| this.refresh_models(window, cx)),
                cx,
            ),
        ]
    }

    /// The API key reads as a field in both states: a masked key or a
    /// placeholder in an input frame, with a small outline button to edit.
    fn render_api_key(
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
                            .max_h(px(320.))
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
                            .font_family(FONT_MONO)
                            .text_size(px(TEXT_MONO_SMALL))
                            .line_height(px(LINE_SMALL))
                            .text_color(p.muted_foreground)
                            .child(text),
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
                                .on_click(cx.listener(|this, _, _, cx| this.save_prompt(cx))),
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

    fn render_privacy(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let p = cx.palette();
        let s = settings(cx);
        let (files, bytes) = self.storage_usage;
        let usage = if files > 0 {
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
                Some(t("settings.privacy.historyDescription").into()),
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
                Some(t("settings.privacy.audioRetentionDescription").into()),
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
                            .disabled(files == 0)
                            .on_click(cx.listener(|this, _, _, cx| this.confirm_clear_audio(cx))),
                        &p,
                    )
                    .into_any_element(),
                ],
                &p,
            ))
            .child(settings_section(
                t("settings.permissions.title"),
                Some(t("settings.permissions.description").into()),
                vec![
                    permission_row(
                        "settings-perm-mic",
                        t("settings.permissions.microphoneTitle"),
                        t("settings.permissions.microphoneDescription"),
                        self.mic_permission,
                        cx.listener(|this, _, _, cx| {
                            backend::request_microphone_permission();
                            this.mic_permission = backend::microphone_permission_granted();
                            cx.notify();
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
                Some(t("settings.permissions.troubleshootingDescription").into()),
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
    fn confirm_reset_accessibility(&mut self, cx: &mut Context<Self>) {
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
    fn confirm_clear_audio(&mut self, cx: &mut Context<Self>) {
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
                AppState::global(cx).update(cx, |s, cx| s.delete_all_audio(cx));
                let _ = this.update(cx, |this, cx| {
                    this.storage_usage = backend::audio_storage_usage();
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
            },
        );
        request.confirm_label = t("settings.privacy.clearAudioConfirm").into();
        show_dialog(request, cx);
    }
}

/// What Tap mode does with the saved hotkeys. Keys the native listener
/// watches (Globe, a lone right-side modifier) start on a Double tap; key
/// combinations toggle on a single press. Each carries the hotkey's keycap
/// glyphs, or none when several hotkeys share the sentence.
#[derive(Clone, Debug, PartialEq, Eq)]
enum TapStyle {
    DoubleTap(Vec<String>),
    Toggle(Vec<String>),
    /// Some hotkeys double-tap and others toggle.
    Mixed,
}

impl TapStyle {
    /// Names the hotkey in the card copy when there is exactly one.
    fn of(hotkeys: &[Hotkey]) -> Self {
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

    fn keys(&self) -> &[String] {
        match self {
            Self::DoubleTap(keys) | Self::Toggle(keys) => keys,
            Self::Mixed => &[],
        }
    }
}

/// A card sentence whose `{{hotkey}}` is drawn as keycaps, like the Hotkey
/// row above, instead of a run of glyphs squeezed into the prose. Words wrap
/// one at a time so the keycaps flow inline with the text.
fn sentence_with_keys(key: &str, keys: &[String], p: &Palette) -> AnyElement {
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
fn activation_mode_cards(
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
                    // Selection takes the accent, as an open select or a
                    // focused field does.
                    // Both start from the card tone every settings group uses;
                    // selection tints it with the accent rather than dimming it.
                    if selected {
                        d.border_color(p.ring)
                            .bg(p
                                .card
                                .blend(p.primary.opacity(if p.dark { 0.12 } else { 0.06 })))
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
fn move_activation_mode(
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
const STT_MODEL_CHOICES: usize = 12;

/// The 12 px radio used by the activation cards and the model list; a
/// chosen one takes the accent like every other selection.
fn radio_dot(selected: bool, p: &Palette) -> Div {
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
fn icon_action(
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
fn ghost_action(id: &'static str, role: Role, label: String, cx: &mut App) -> Stateful<Div> {
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
fn connection_status(
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
fn connection_row(
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
fn suggestion_chip(
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
fn toggle_row(
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
fn error_line(message: String, p: &Palette) -> AnyElement {
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

/// The host and port of a base URL, as people read it: `127.0.0.1:9`.
pub(super) fn endpoint_host(base: &str) -> String {
    url::Url::parse(base)
        .ok()
        .and_then(|url| {
            let host = url.host_str()?.to_string();
            Some(match url.port() {
                Some(port) => format!("{host}:{port}"),
                None => host,
            })
        })
        .unwrap_or_else(|| base.to_string())
}

/// Turns a server error into a sentence with the next step, plus the raw
/// text when it adds detail. Messages that are already localized pass
/// through as they are.
fn friendly_server_error(raw: &str, base: &str) -> (String, Option<String>) {
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

fn info_line(label: String, value: String, p: &Palette) -> Div {
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

/// The models the server listed, as a table whose rows fill the Model
/// field; the row matching the field is marked.
fn model_card_list(
    models: &[ModelOption],
    selected: &str,
    this: WeakEntity<SettingsModal>,
    p: &Palette,
    cx: &mut App,
) -> AnyElement {
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
            .text_color(p.subtle_foreground)
            .child(text)
    };
    let mut table = div()
        .flex()
        .flex_col()
        .rounded(px(RADIUS_LG))
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
                .text_color(p.subtle_foreground)
                .child(text)
        };
        table = table.child(
            row()
                .id(("model-card", index))
                .keyboard_focusable(
                    ("settings-model", index),
                    Role::RadioButton,
                    model.label.clone(),
                    cx,
                )
                .aria_toggled(if is_selected {
                    Toggled::True
                } else {
                    Toggled::False
                })
                .py(px(9.))
                .when(index > 0, |d| d.border_t_1().border_color(line))
                .cursor_pointer()
                .when(is_selected, |d| d.bg(p.hover_wash(0.06)))
                .when(!is_selected, |d| d.hover(|s| s.bg(p.hover_wash(0.03))))
                .on_click({
                    let this = this.clone();
                    move |_, window, cx| {
                        let value = value.clone();
                        let _ =
                            this.update(cx, |this, cx| this.pick_cleanup_model(value, window, cx));
                    }
                })
                .child(radio_dot(is_selected, &p))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(
                            div()
                                .flex()
                                .overflow_hidden()
                                .font_family(FONT_MONO)
                                .text_size(px(TEXT_MONO_SMALL))
                                .font_weight(FontWeight::MEDIUM)
                                .when(!source.is_empty(), |d| {
                                    d.child(
                                        div()
                                            .flex_shrink(1.)
                                            .min_w_0()
                                            .truncate()
                                            .text_color(p.subtle_foreground)
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
                                    .text_color(p.subtle_foreground)
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
                    .font_family(FONT_MONO)
                    .text_size(px(TEXT_MONO_CAPTION)),
                )
                .child(
                    muted(model.owned_by.clone().unwrap_or_else(|| "—".to_string()))
                        .w(px(MODEL_OWNER_W)),
                ),
        );
    }
    table.into_any_element()
}

fn permission_row(
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

impl Focusable for SettingsModal {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for SettingsModal {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = cx.palette();
        let frame = sheet_frame(window.viewport_size());
        let nav = self.render_nav(frame.attached, cx);
        let content = match self.section {
            SettingsSection::General => self.render_general(cx),
            SettingsSection::Hotkeys => self.render_hotkeys(cx),
            SettingsSection::SpeechToText => self.render_speech_to_text(window, cx),
            SettingsSection::Llms => self.render_llms(window, cx),
            SettingsSection::PrivacyData => self.render_privacy(cx),
        };
        let closing = self.closing;
        let reduce_motion = cx.reduce_motion();
        let section_label = t(&format!("settings.sections.{}.label", self.section.key()));
        // Over the full window the header is the window's top bar: as tall,
        // and it moves the window.
        let header = if frame.attached {
            drag_region().h(px(WORKSPACE_TOP_BAR_HEIGHT))
        } else {
            div().h(px(48.))
        };
        let header = header
            .flex_none()
            .flex()
            .items_center()
            .gap(px(8.))
            // The breadcrumb starts where the main top bar's title does
            // (its padding, sidebar toggle and gap), so opening Settings
            // never shifts the title sideways.
            .pl(px(if frame.compact { 20. } else { 50. }))
            .pr(px(10.))
            // No rule under it, like the main top bar it stands in for.
            .text_size(px(TEXT_BODY))
            .font_weight(FontWeight::MEDIUM)
            .child(
                div()
                    .text_color(p.muted_foreground)
                    .child(t("sidebar.settings")),
            )
            .child(div().text_color(p.subtle_foreground).child("›"))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(p.foreground)
                    .child(section_label),
            )
            .child(
                Button::new("settings-close")
                    .icon("x", 16.)
                    .aria_label(t("settings.close"))
                    .variant(ButtonVariant::Ghost)
                    .size(ButtonSize::Sm)
                    .on_click(cx.listener(|this, _, window, cx| this.close(window, cx))),
            );

        // A short soft edge along the sheet's bottom, with the overlay scroll
        // thumb, says the page continues below without washing out the last
        // visible row. It spans the whole column, edge to edge, so it reads
        // as the sheet's edge rather than a fade inside whichever card sits
        // there.
        let max_scroll = self.scroll.max_offset().y;
        // The scroll extent is only known after layout; look again next frame
        // so the fade shows on a page that opens with more below.
        if self.measured_section != Some(self.section) {
            self.measured_section = Some(self.section);
            cx.on_next_frame(window, |_, _, cx| cx.notify());
        }
        let more_below = max_scroll > px(1.) && self.scroll.offset().y > -max_scroll + px(1.);
        let fade = f32_tx(
            "settings-scroll-fade",
            if more_below { 1. } else { 0. },
            DUR_FAST,
            ease_out,
            window,
            cx,
        );
        let radius = px(RADIUS_2XL - 1.);
        let body = div()
            .relative()
            .flex_1()
            .min_h_0()
            .child(
                div()
                    .id("settings-content")
                    .size_full()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .child(
                        page_container()
                            .when(frame.compact, |d| d.px(px(20.)))
                            .pb(px(48.))
                            .child(content),
                    ),
            )
            .when_some(scroll_thumb(&self.scroll), |d, (top, height)| {
                d.child(
                    div()
                        .absolute()
                        .right(px(3.))
                        .top(top)
                        .w(px(4.))
                        .h(height)
                        .rounded_full()
                        .bg(p.foreground.opacity(if p.dark { 0.22 } else { 0.18 })),
                )
            })
            .when(fade > 0.01, |d| {
                d.child(
                    div()
                        .absolute()
                        .left_0()
                        .right_0()
                        .bottom_0()
                        .h(px(16.))
                        .opacity(fade)
                        .when(!frame.attached, |d| d.rounded_br(radius))
                        .bg(linear_gradient(
                            0.,
                            linear_color_stop(p.background, 0.),
                            linear_color_stop(p.background.opacity(0.), 1.),
                        )),
                )
            });

        let panel = div()
            .id("settings-panel")
            .role(Role::Dialog)
            .aria_label(t("sidebar.settings"))
            .key_context("SettingsModal")
            .on_action(cx.listener(Self::on_close_overlay))
            .occlude()
            .absolute()
            .left(frame.bounds.origin.x)
            .top(frame.bounds.origin.y)
            .w(frame.bounds.size.width)
            .h(frame.bounds.size.height)
            .flex()
            // Over the full window the window's own corners and edge frame it.
            .when(!frame.attached, |d| d.rounded(px(RADIUS_2XL)).border_1())
            .overflow_hidden()
            .border_color(p.border)
            .bg(p.background)
            .shadow(dialog_shadow(&p))
            .on_click(|_, _, cx| cx.stop_propagation())
            .child(
                div()
                    // The app sidebar's width, so when the sheet hangs from
                    // the top bar the two dividers continue as one line.
                    .w(px(if frame.compact { 196. } else { SIDEBAR_WIDTH }))
                    .flex_none()
                    .h_full()
                    // The panel clips square, so the sidebar fill rounds its
                    // own outer corners to sit inside the border.
                    .when(!frame.attached, |d| d.rounded_l(radius))
                    .border_r_1()
                    .border_color(p.border)
                    .bg(p.settings_sidebar)
                    .child(nav),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .flex()
                    .flex_col()
                    .child(header)
                    .child(body),
            )
            .focus_trap("settings-sheet", &self.focus)
            .with_animation(
                ("settings-panel-anim", closing as usize),
                motion(if closing { DUR_SNAP } else { DUR_BASE }, cx).with_easing(if closing {
                    ease_snap
                } else {
                    ease_out
                }),
                move |d, t| {
                    let v = if closing { 1. - t } else { t };
                    // zoom-in-98.5: approximate the scale with a small lift,
                    // which Reduce motion drops.
                    let d = d.opacity(v);
                    if reduce_motion {
                        d
                    } else {
                        d.mt(px(4. * (1. - v)))
                    }
                },
            );

        div()
            .id("settings-backdrop")
            .absolute()
            .inset_0()
            .bg(backdrop(&p))
            .occlude()
            .on_click(cx.listener(|this, _, window, cx| this.close(window, cx)))
            .child(panel)
            .with_animation(
                ("settings-backdrop-anim", closing as usize),
                motion(if closing { DUR_SNAP } else { DUR_BASE }, cx).with_easing(if closing {
                    ease_snap
                } else {
                    ease_out
                }),
                move |d, t| d.opacity(if closing { 1. - t } else { t }),
            )
    }
}

/// The overlay scroll thumb's top and height inside the content viewport,
/// when the page is taller than the sheet. It stays visible rather than
/// fading like a trackpad scroller, so a clipped row reads as more below.
fn scroll_thumb(scroll: &ScrollHandle) -> Option<(Pixels, Pixels)> {
    const INSET: f32 = 3.;
    const MIN_THUMB: f32 = 32.;
    let viewport = f32::from(scroll.bounds().size.height);
    let max = f32::from(scroll.max_offset().y);
    if max < 1. || viewport <= 2. * INSET + MIN_THUMB {
        return None;
    }
    let track = viewport - 2. * INSET;
    let height = (track * viewport / (viewport + max)).max(MIN_THUMB);
    let progress = (-f32::from(scroll.offset().y) / max).clamp(0., 1.);
    Some((px(INSET + (track - height) * progress), px(height)))
}

/// Where the sheet sits in the window, and whether it uses the narrow layout.
struct SheetFrame {
    bounds: Bounds<Pixels>,
    /// Covers the whole window, top bar included, instead of floating
    /// centered; the traffic lights sit over its navigation.
    attached: bool,
    compact: bool,
}

/// The widest the sheet grows; its content column stops at `PAGE_MAX_WIDTH`.
const SHEET_MAX_WIDTH: f32 = 1120.;
const SHEET_MAX_HEIGHT: f32 = 960.;

/// A centered sheet when the window is wide enough for it to clear the app
/// sidebar entirely. Otherwise the sheet covers the whole window, top bar
/// included, so no page title, search field, sidebar label or transcript
/// line peeks out dimmed beside it.
fn sheet_frame(viewport: Size<Pixels>) -> SheetFrame {
    let (vw, vh) = (f32::from(viewport.width), f32::from(viewport.height));
    let centered_inset = (vw - SHEET_MAX_WIDTH) / 2.;
    let attached = centered_inset < SIDEBAR_WIDTH + 24.;
    let (x, y, w, h) = if attached {
        (0., 0., vw, vh)
    } else {
        let h = (vh - 2. * WORKSPACE_TOP_BAR_HEIGHT).min(SHEET_MAX_HEIGHT);
        (centered_inset, (vh - h) / 2., SHEET_MAX_WIDTH, h)
    };
    SheetFrame {
        bounds: Bounds::new(point(px(x), px(y)), size(px(w), px(h))),
        attached,
        compact: w < 900.,
    }
}

#[cfg(test)]
mod tests {
    use super::{TapStyle, endpoint_host, friendly_server_error, sheet_frame};
    use crate::text::hotkeys::parse_hotkey_list;
    use gpui_kit::{px, size};

    #[test]
    fn transport_errors_become_a_next_step_and_keep_the_raw_text() {
        let raw = "error sending request for url (http://127.0.0.1:9/v1/models)";
        let (headline, details) = friendly_server_error(raw, "http://127.0.0.1:9/v1");
        assert!(headline.contains("127.0.0.1:9"));
        assert_eq!(details.as_deref(), Some(raw));
    }

    #[test]
    fn status_errors_name_the_code_and_localized_messages_pass_through() {
        let (headline, details) = friendly_server_error("404 Not Found", "http://host/v1");
        assert!(headline.contains("404"));
        assert!(details.is_some());
        let (headline, details) = friendly_server_error("Add an API key.", "http://host/v1");
        assert_eq!(headline, "Add an API key.");
        assert!(details.is_none());
    }

    #[test]
    fn endpoint_host_keeps_the_port() {
        assert_eq!(
            endpoint_host("http://localhost:11434/v1"),
            "localhost:11434"
        );
        assert_eq!(endpoint_host("https://api.openai.com/v1"), "api.openai.com");
    }

    #[test]
    fn sheet_covers_the_sidebar_unless_it_can_clear_it() {
        let narrow = sheet_frame(size(px(720.), px(480.)));
        assert!(narrow.attached && narrow.compact);
        assert_eq!(narrow.bounds.origin.x, px(0.));
        let default = sheet_frame(size(px(1200.), px(800.)));
        assert!(default.attached && !default.compact);
        // No dimmed strip of the top bar shows above it.
        assert_eq!(default.bounds.origin.y, px(0.));
        assert_eq!(default.bounds.size.height, px(800.));
        let wide = sheet_frame(size(px(1800.), px(1100.)));
        assert!(!wide.attached);
        assert!(f32::from(wide.bounds.origin.x) >= super::SIDEBAR_WIDTH + 24.);
    }

    #[test]
    fn tap_mode_names_what_the_saved_hotkey_does() {
        assert_eq!(
            TapStyle::of(&parse_hotkey_list("RightOption")),
            TapStyle::DoubleTap(vec!["Right ⌥".into()])
        );
        assert_eq!(
            TapStyle::of(&parse_hotkey_list("Control+Shift+K")),
            TapStyle::Toggle(vec!["⌃".into(), "⇧".into(), "K".into()])
        );
        assert_eq!(
            TapStyle::of(&parse_hotkey_list("RightOption,Control+Shift+K")),
            TapStyle::Mixed
        );
    }
}
