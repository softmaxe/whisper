//! The Settings modal and the panels it hosts.

mod controls;
mod general;
mod microphone;
mod privacy;
mod prompt_studio;
mod servers;

use controls::*;
use microphone::{microphone_status_info, refresh_microphones};

use std::time::{Duration, Instant};

use std::rc::Rc;

use std::future::Future;

use gpui_kit::base::FocusTrapElement as _;
use gpui_kit::component::input::{InputEvent, InputState, TextareaState};
use gpui_kit::component::text::{TextView, TextViewState};
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
use crate::audio::capture::CaptureStatus;
use crate::backend::{self, AutoStartState};
use crate::dictation::Dictation;
use crate::native::mic::{MicDevice, MicSnapshot};
use crate::services::cleanup::ModelOption;
use crate::text::prompts::default_cleanup_prompt;
use crate::text::url::normalize_base_url;

use super::components::f32_tx;
use super::components::{
    Button, ButtonSize, ButtonVariant, KeyboardFocusable, NavSurface, Toggle, backdrop,
    dialog_shadow, icon, kbd, keyed_focus_handle, nav_row, page_container, panel_row, scroll_thumb,
    section_header, segment, segmented_track, settings_field, settings_field_tagged, settings_row,
    settings_section, status_label,
};
use super::control_panel::{CloseOverlay, SIDEBAR_WIDTH, drag_region};
use super::hotkey_input::{HotkeyListInput, hotkey_glyphs};
use super::overlays::{DialogRequest, Overlays, Toast, ToastVariant, show_dialog, show_toast};
use super::scroll::{consume_delta, wheel_region};
use super::select::{Select, SelectEvent, SelectOption};
use super::text_input::{text_field, text_field_disabled, textarea_field};
use super::theme::{
    DUR_BASE, DUR_FAST, DUR_SNAP, FONT_MONO, LINE_READING, LINE_SMALL, Palette, PaletteExt,
    RADIUS_2XL, RADIUS_LG, RADIUS_MD, RADIUS_SM, RADIUS_XL, ROW_CONTROL_HEIGHT, TEXT_BODY,
    TEXT_CAPTION, TEXT_MONO_BODY, TEXT_MONO_SMALL, TEXT_SMALL, WORKSPACE_TOP_BAR_HEIGHT,
    cubic_bezier, ease_out, ease_snap, motion, progress,
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
    fn connected(models: &[ModelOption], model: &str) -> Self {
        Self::Connected {
            model_listed: (!model.is_empty() && !models.is_empty())
                .then(|| models.iter().any(|m| m.matches(model))),
        }
    }
}

/// One width for every select in a settings row, so the control column keeps
/// a single left edge down the page.
const SELECT_WIDTH: f32 = 200.;

/// The input device select grows past `SELECT_WIDTH` up to this cap for long
/// device names; longer ones truncate and show in full in the menu.
const MIC_SELECT_WIDTH: f32 = 240.;
const MIC_REFRESH_ANIMATION_MS: u64 = 600;

const RETENTION_DAY_OPTIONS: [u32; 6] = [1, 7, 14, 30, 60, 90];

pub struct SettingsModal {
    section: SettingsSection,
    closing: bool,

    stt_url: Entity<InputState>,
    stt_model: Entity<InputState>,

    cleanup_url: Entity<InputState>,
    cleanup_model: Entity<Select>,
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
    prompt_preview: Entity<TextViewState>,
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
    stt_models: Vec<ModelOption>,
    stt_request: Option<Task<()>>,

    auto_start: AutoStartState,
    mic_snapshot: MicSnapshot,
    mic_capture_status: CaptureStatus,
    mic_refresh: async_channel::Sender<()>,
    mic_refresh_started: Option<Instant>,
    _mic_monitor: Task<()>,
    storage_usage: (usize, u64),
    storage_usage_generation: u64,
    storage_usage_loading: bool,
    clearing_audio: bool,
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

/// Only settings that feed the select values, labels, or enabled state require
/// rebuilding their options. Recording and History notifications still repaint.
fn select_settings_changed(before: &Settings, after: &Settings) -> bool {
    before.ui_language != after.ui_language
        || before.preferred_language != after.preferred_language
        || before.chinese_script_preference != after.chinese_script_preference
        || before.panel_start_position != after.panel_start_position
        || before.audio_retention_days != after.audio_retention_days
        || before.transcript_retention_days != after.transcript_retention_days
        || before.data_retention_enabled != after.data_retention_enabled
        || before.microphone_selection_mode != after.microphone_selection_mode
        || before.selected_mic_device_id != after.selected_mic_device_id
        || before.selected_mic_device_label != after.selected_mic_device_label
}

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
    fn scroll_page(&self, delta: Pixels) -> bool {
        let mut offset = self.scroll.offset();
        let (next, _) = consume_delta(offset.y, self.scroll.max_offset().y, delta);
        if next == offset.y {
            return false;
        }
        offset.y = next;
        self.scroll.set_offset(offset);
        true
    }

    /// The workspace remains visible while a floating sheet or exit animation is shown.
    pub(super) fn covers_workspace(&self, viewport: Size<Pixels>) -> bool {
        !self.closing && sheet_frame(viewport).attached
    }

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
            let selected = (!s.cleanup_model.trim().is_empty())
                .then(|| s.cleanup_model.trim().to_string().into());
            let mut select = Select::new(Vec::new(), selected, window, cx)
                .with_label(t("common.model"))
                .with_custom_value(window, cx);
            select.set_placeholder(t("reasoning.custom.modelPlaceholder"));
            select
        });
        subscriptions.push(cx.subscribe(&cleanup_model, |_, _, e: &SelectEvent, cx| {
            let SelectEvent::Change(value) = e;
            let value = value.trim().to_string();
            update_settings(cx, |s| s.cleanup_model = value);
            cx.notify();
        }));

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
        #[cfg(debug_assertions)]
        if std::env::var("WHISPER_DEV_SCENE").as_deref() == Ok("settings:language") {
            let select = ui_language.clone();
            window.on_next_frame(move |window, cx| {
                select.update(cx, |select, cx| select.preview_open(window, cx));
            });
        }
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
            } else if let Some(device) = this
                .mic_snapshot
                .devices
                .iter()
                .find(|d| d.id == value.as_ref())
            {
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
        let mut synced_settings = s.clone();
        subscriptions.push(cx.observe(&state, move |this, state, cx| {
            let current = &state.read(cx).settings;
            if select_settings_changed(&synced_settings, current) {
                synced_settings = current.clone();
                this.sync_selects(cx);
            }
            // Other state changes still affect the sheet's rendered status.
            cx.notify();
        }));

        let (mic_refresh, mic_requests) = async_channel::bounded(1);
        // One serial worker handles both automatic and manual refreshes. The
        // task holds a weak entity and is cancelled when the sheet is dropped.
        let mic_monitor = cx.spawn(async move |this, cx| {
            let executor = cx.background_executor().clone();
            refresh_microphones(
                mic_requests,
                || executor.spawn(async { backend::microphone_snapshot() }),
                |snapshot: MicSnapshot| {
                    this.update(cx, |this: &mut Self, cx| {
                        let capture = Dictation::global(cx).read(cx).capture_status();
                        let devices_changed = this.mic_snapshot.devices != snapshot.devices;
                        let changed =
                            this.mic_snapshot != snapshot || this.mic_capture_status != capture;
                        this.mic_snapshot = snapshot;
                        this.mic_capture_status = capture;
                        if devices_changed {
                            this.sync_microphone_select(cx);
                        }
                        if changed {
                            cx.notify();
                        }
                    })
                    .is_ok()
                },
                || executor.timer(Duration::from_secs(1)),
            )
            .await;
        });

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
            prompt_preview: cx.new(|cx| TextViewState::markdown("", cx)),
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
            mic_snapshot: MicSnapshot::default(),
            mic_capture_status: CaptureStatus::Idle,
            mic_refresh,
            mic_refresh_started: None,
            _mic_monitor: mic_monitor,
            storage_usage: (0, 0),
            storage_usage_generation: 0,
            storage_usage_loading: false,
            clearing_audio: false,
            mic_permission: backend::microphone_permission_granted(),
            accessibility_permission: backend::accessibility_granted(),
            scroll: ScrollHandle::new(),
            measured_section: None,
            focus,
            restore_focus: window.focused(cx),
            _subscriptions: subscriptions,
        };
        if section == SettingsSection::PrivacyData {
            this.load_storage_usage(cx);
        }
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
                self.load_storage_usage(cx);
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
            SettingsSection::Hotkeys => self.render_hotkeys(window, cx),
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
            cx.on_next_frame(window, |this, _, cx| {
                #[cfg(debug_assertions)]
                if let Ok(fraction) = std::env::var("WHISPER_DEV_SCROLL_FRACTION")
                    && let Ok(fraction) = fraction.parse::<f32>()
                {
                    let extent = this.scroll.max_offset().y;
                    this.scroll
                        .set_offset(point(px(0.), -extent * fraction.clamp(0., 1.)));
                }
                #[cfg(not(debug_assertions))]
                let _ = this;
                cx.notify();
            });
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
                    .relative()
                    .size_full()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .child(wheel_region(
                        DispatchPhase::Bubble,
                        Some(self.scroll.clone()),
                        cx.listener(|this, event: &ScrollWheelEvent, window, cx| {
                            let delta = event.delta.pixel_delta(window.line_height()).y;
                            if this.scroll_page(delta) {
                                cx.notify();
                            }
                            cx.stop_propagation();
                        }),
                    ))
                    .child(
                        page_container()
                            .max_w(px(860.))
                            .pt(px(if frame.compact { 20. } else { 32. }))
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
    use super::{
        ConnectionCheck, TapStyle, cleanup_model_options, endpoint_host, friendly_server_error,
        microphone_status_info, refresh_microphones, sheet_frame,
    };
    use crate::audio::capture::CaptureStatus;
    use crate::native::mic::{MicDevice, MicSnapshot};
    use crate::services::cleanup::ModelOption;
    use crate::settings::{MicrophoneSelectionMode, Settings};
    use crate::text::hotkeys::parse_hotkey_list;
    use futures::FutureExt;
    use gpui_kit::{px, size};
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    #[test]
    fn select_sync_ignores_unrelated_settings_but_tracks_values_and_localization() {
        let baseline = Settings::default();
        let unrelated = Settings {
            cleanup_model: "different-model".into(),
            remote_transcription_url: "http://localhost:8000/v1".into(),
            auto_paste_enabled: !baseline.auto_paste_enabled,
            ..baseline.clone()
        };
        assert!(!super::select_settings_changed(&baseline, &unrelated));
        let changes: [fn(&mut Settings); 10] = [
            |s| s.ui_language = "zh-CN".into(),
            |s| s.preferred_language = "en".into(),
            |s| s.chinese_script_preference = crate::settings::ChineseScriptPreference::Traditional,
            |s| s.panel_start_position = crate::settings::PanelStartPosition::Center,
            |s| s.audio_retention_days += 1,
            |s| s.transcript_retention_days += 1,
            |s| s.data_retention_enabled = !s.data_retention_enabled,
            |s| s.microphone_selection_mode = MicrophoneSelectionMode::Specific,
            |s| s.selected_mic_device_id = "external-device".into(),
            |s| s.selected_mic_device_label = "External microphone".into(),
        ];
        for change in changes {
            let mut changed = unrelated.clone();
            change(&mut changed);
            assert!(super::select_settings_changed(&baseline, &changed));
            assert!(!super::select_settings_changed(&changed, &changed));
        }
    }

    #[test]
    fn cleanup_picker_preserves_saved_aliases_and_searches_all_api_names() {
        let models = vec![ModelOption {
            value: "team/Model".into(),
            aliases: vec!["cleanup".into(), "other-alias".into()],
            owned_by: Some("vllm".into()),
        }];
        let options = cleanup_model_options(&models, "cleanup");
        assert_eq!(options.len(), 1);
        assert_eq!(options[0].value.as_ref(), "cleanup");
        assert_eq!(options[0].label.as_ref(), "cleanup");
        assert_eq!(
            options[0].keywords.as_deref(),
            Some("team/Model cleanup other-alias")
        );
        let options = cleanup_model_options(&models, "unlisted");
        assert_eq!(options[0].value.as_ref(), "team/Model");
    }

    #[test]
    fn connection_check_recognizes_grouped_model_aliases() {
        let models = vec![ModelOption {
            value: "Qwen/Qwen3-ASR-1.7B".into(),
            aliases: vec!["Qwen3-ASR-1.7B".into(), "whisper-1".into()],
            owned_by: Some("vllm".into()),
        }];
        for name in ["Qwen/Qwen3-ASR-1.7B", "Qwen3-ASR-1.7B", "whisper-1"] {
            assert!(matches!(
                ConnectionCheck::connected(&models, name),
                ConnectionCheck::Connected {
                    model_listed: Some(true)
                }
            ));
        }
        assert!(matches!(
            ConnectionCheck::connected(&models, "missing"),
            ConnectionCheck::Connected {
                model_listed: Some(false)
            }
        ));
        assert!(matches!(
            ConnectionCheck::connected(&models, ""),
            ConnectionCheck::Connected { model_listed: None }
        ));
        assert!(matches!(
            ConnectionCheck::connected(&[], "whisper-1"),
            ConnectionCheck::Connected { model_listed: None }
        ));
    }

    fn refresh_worker(
        requests: async_channel::Receiver<()>,
        results: async_channel::Receiver<usize>,
        ticks: async_channel::Receiver<()>,
        scans: Rc<Cell<usize>>,
        published: Rc<RefCell<Vec<usize>>>,
    ) -> impl std::future::Future<Output = ()> {
        refresh_microphones(
            requests,
            move || {
                scans.set(scans.get() + 1);
                let results = results.clone();
                async move { results.recv().await.unwrap() }
            },
            move |sample| {
                published.borrow_mut().push(sample);
                true
            },
            move || {
                let ticks = ticks.clone();
                async move { ticks.recv().await.unwrap() }
            },
        )
    }

    #[test]
    fn microphone_refresh_serializes_slow_scans_and_coalesces_manual_requests() {
        let (refresh, requests) = async_channel::bounded(1);
        let (complete, results) = async_channel::unbounded();
        let (tick, ticks) = async_channel::unbounded();
        let scans = Rc::new(Cell::new(0));
        let published = Rc::new(RefCell::new(Vec::new()));
        let mut worker = Box::pin(refresh_worker(
            requests,
            results,
            ticks,
            scans.clone(),
            published.clone(),
        ));
        assert!(worker.as_mut().now_or_never().is_none());
        assert_eq!(scans.get(), 1);
        refresh.try_send(()).unwrap();
        assert!(refresh.try_send(()).is_err());
        assert!(worker.as_mut().now_or_never().is_none());
        assert_eq!(
            scans.get(),
            1,
            "a manual request cannot overlap a slow scan"
        );
        complete.try_send(1).unwrap();
        assert!(worker.as_mut().now_or_never().is_none());
        assert_eq!(scans.get(), 2, "one coalesced follow-up scan");
        complete.try_send(2).unwrap();
        assert!(worker.as_mut().now_or_never().is_none());
        assert_eq!(*published.borrow(), vec![1, 2]);
        assert_eq!(scans.get(), 2);
        tick.try_send(()).unwrap();
        assert!(worker.as_mut().now_or_never().is_none());
        assert_eq!(
            scans.get(),
            3,
            "automatic refresh works without a button press"
        );
        complete.try_send(3).unwrap();
        drop(refresh);
        assert_eq!(worker.as_mut().now_or_never(), Some(()));
        assert_eq!(*published.borrow(), vec![1, 2, 3]);
    }

    #[test]
    fn closing_microphone_refresh_drops_inflight_results_and_reopening_starts_fresh() {
        let published = Rc::new(RefCell::new(Vec::new()));
        for reopen in 0..2 {
            let (_refresh, requests) = async_channel::bounded(1);
            let (complete, results) = async_channel::unbounded();
            let (_tick, ticks) = async_channel::unbounded();
            let scans = Rc::new(Cell::new(0));
            let mut worker = Box::pin(refresh_worker(
                requests,
                results,
                ticks,
                scans.clone(),
                published.clone(),
            ));
            assert!(worker.as_mut().now_or_never().is_none());
            assert_eq!(scans.get(), 1);
            if reopen == 1 {
                complete.try_send(42).unwrap();
                assert!(worker.as_mut().now_or_never().is_none());
                assert_eq!(*published.borrow(), vec![42]);
            }
            drop(worker);
            assert!(
                complete.try_send(99).is_err(),
                "closed sheets cannot receive late scan results"
            );
            assert_eq!(scans.get(), 1, "closed sheets cannot start another scan");
        }
        assert_eq!(*published.borrow(), vec![42]);
    }

    #[test]
    fn microphone_preview_updates_while_live_device_stays_with_its_session() {
        let mut snapshot = MicSnapshot {
            devices: vec![
                MicDevice {
                    id: "mac".into(),
                    label: "MacBook Microphone".into(),
                    is_built_in: true,
                    is_default: true,
                },
                MicDevice {
                    id: "phone".into(),
                    label: "iPhone Microphone".into(),
                    is_built_in: false,
                    is_default: false,
                },
            ],
            system_default_name: Some("MacBook Microphone".into()),
            lid_closed: Some(true),
        };
        let mut settings = Settings::default();
        assert_eq!(
            microphone_status_info(&snapshot, &settings, &CaptureStatus::Idle),
            (
                "microphoneSettings.nextRecordingUses",
                "iPhone Microphone".into()
            )
        );
        let live = CaptureStatus::Recording("iPhone Microphone".into());
        snapshot.lid_closed = Some(false);
        settings.microphone_selection_mode = MicrophoneSelectionMode::Specific;
        settings.selected_mic_device_id = "mac".into();
        assert_eq!(
            microphone_status_info(&snapshot, &settings, &live),
            (
                "microphoneSettings.currentlyUsing",
                "iPhone Microphone".into()
            )
        );
        assert_eq!(
            microphone_status_info(&snapshot, &settings, &CaptureStatus::Idle),
            (
                "microphoneSettings.nextRecordingUses",
                "MacBook Microphone".into()
            )
        );
    }

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
