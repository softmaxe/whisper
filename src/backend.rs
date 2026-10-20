//! The UI's view of the platform and services. Each function wraps one call
//! into the library (`crate::native`, `crate::services`, ...) so the views
//! stay free of platform details.

use std::path::PathBuf;

use crate::native::permissions::SettingsPane;

#[derive(Clone, Debug, PartialEq)]
pub struct MicDeviceInfo {
    pub id: String,
    pub label: String,
    pub is_built_in: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ModelOption {
    pub value: String,
    pub label: String,
    pub description: Option<String>,
    pub owner: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AutoStartState {
    pub enabled: bool,
    pub requires_approval: bool,
}

pub fn auto_start_state() -> AutoStartState {
    let state = crate::native::autostart::auto_start_state();
    AutoStartState {
        enabled: state.enabled,
        requires_approval: state.requires_approval,
    }
}

pub fn set_auto_start(enabled: bool) -> anyhow::Result<()> {
    crate::native::autostart::set_auto_start_enabled(enabled).map_err(anyhow::Error::msg)
}

pub fn open_login_items_settings() {
    open_url(SettingsPane::LoginItems.url());
}

pub fn open_microphone_privacy_settings() {
    open_url(SettingsPane::Microphone.url());
}

pub fn open_accessibility_settings() {
    open_url(SettingsPane::Accessibility.url());
}

pub fn open_url(url: &str) {
    let _ = std::process::Command::new("open").arg(url).spawn();
}

pub fn reveal_in_finder(path: &PathBuf) {
    let _ = std::process::Command::new("open")
        .arg("-R")
        .arg(path)
        .spawn();
}

pub fn list_microphones() -> Vec<MicDeviceInfo> {
    crate::native::mic::list_input_devices()
        .into_iter()
        .map(|d| MicDeviceInfo {
            id: d.id,
            label: d.label,
            is_built_in: d.is_built_in,
        })
        .collect()
}

pub fn system_default_microphone() -> Option<String> {
    crate::native::mic::system_default_microphone().map(|d| d.name)
}

pub fn lid_closed() -> Option<bool> {
    crate::native::mic::LaptopLidMonitor::shared().state()
}

pub fn microphone_permission_granted() -> bool {
    crate::native::permissions::microphone_access()
        == crate::native::permissions::MicrophoneAccess::Granted
}

pub fn request_microphone_permission() {
    use crate::native::permissions::{MicrophoneAccess, microphone_access};
    match microphone_access() {
        MicrophoneAccess::NotDetermined => {
            std::thread::spawn(crate::native::permissions::request_microphone_access);
        }
        MicrophoneAccess::Granted => {}
        _ => open_microphone_privacy_settings(),
    }
}

pub fn accessibility_granted() -> bool {
    crate::native::permissions::is_accessibility_trusted(false)
}

pub fn request_accessibility_permission() {
    if !crate::native::permissions::is_accessibility_trusted(true) {
        open_accessibility_settings();
    }
}

pub fn audio_storage_usage() -> (usize, u64) {
    let usage = crate::audio_storage::AudioStorage::default().storage_usage();
    (usage.file_count, usage.total_bytes)
}

/// Deletes every stored recording and clears the History rows' audio flags.
pub fn delete_all_audio(db: &crate::db::Database) {
    crate::audio_storage::AudioStorage::default().delete_all_audio_and_flags(db);
}

pub fn cleanup_api_key() -> String {
    crate::secrets::get_cleanup_api_key()
        .ok()
        .flatten()
        .unwrap_or_default()
}

pub fn set_cleanup_api_key(key: &str) {
    if let Err(err) = crate::secrets::set_cleanup_api_key(key) {
        log::warn!("failed to save the cleanup API key: {err}");
    }
}

/// Transcription languages as (code, label, flag) for the language selector.
pub fn languages() -> Vec<(String, String, String)> {
    crate::text::language::languages()
        .iter()
        .map(|l| (l.code.to_string(), l.label.to_string(), l.flag.to_string()))
        .collect()
}

pub fn default_cleanup_prompt(ui_language: &str) -> String {
    crate::text::prompts::default_cleanup_prompt(ui_language).to_string()
}

pub fn normalize_base_url(value: &str) -> String {
    crate::text::url::normalize_base_url(value)
}

pub fn build_api_url(base: &str, path: &str) -> Option<String> {
    crate::text::url::build_api_url(base, path)
}

pub fn format_bytes(bytes: u64) -> String {
    crate::text::format::format_bytes(bytes as f64, 2)
}

#[derive(Clone, Debug, Default)]
pub struct HotkeyModeInfo {
    pub push_disabled_reason: Option<String>,
}

pub fn hotkey_mode_info(hotkeys_list: &str) -> HotkeyModeInfo {
    let Some(manager) = hotkeys() else {
        return HotkeyModeInfo::default();
    };
    let info = manager.mode_info(hotkeys_list);
    HotkeyModeInfo {
        push_disabled_reason: (!info.supports_push_to_talk).then(|| {
            info.push_to_talk_unavailable_reason
                .map(|key| crate::i18n::t(&key))
                .unwrap_or_else(|| crate::i18n::t("hotkey.errors.holdUnavailable"))
        }),
    }
}

pub fn list_models(
    base: String,
    api_key: String,
) -> impl std::future::Future<Output = Result<(String, Vec<ModelOption>), String>> + 'static {
    let handle = crate::services::runtime::spawn(async move {
        let key = (!api_key.is_empty()).then_some(api_key.as_str());
        crate::services::cleanup::list_models(&base, key).await
    });
    async move {
        match handle.await {
            Ok(Ok((resolved, models))) => Ok((
                resolved,
                models
                    .into_iter()
                    .map(|m| ModelOption {
                        description: m.description,
                        owner: m.owned_by,
                        value: m.value,
                        label: m.label,
                    })
                    .collect(),
            )),
            Ok(Err(err)) => Err(err.message_key.map(crate::i18n::t).unwrap_or(err.message)),
            Err(err) => Err(err.to_string()),
        }
    }
}

/// Runs Prompt Studio's test: the edited prompt against the saved server.
pub fn run_cleanup_test(
    settings: crate::settings::Settings,
    api_key: String,
    template: String,
    input: String,
    dictionary_hints: Vec<String>,
) -> impl std::future::Future<Output = Result<String, String>> + 'static {
    use crate::text::{chinese_script, prompts};
    let system_prompt = prompts::resolve_prompt_template(
        &template,
        Some(crate::text::DEFAULT_AGENT_NAME),
        &chinese_script::resolve_cleanup_language(&settings.preferred_language),
        &dictionary_hints,
        &settings.ui_language,
    );
    let config = crate::services::cleanup::CleanupRequestConfig {
        base_url: settings.cleanup_remote_url.trim().to_string(),
        api_key: (!api_key.trim().is_empty()).then(|| api_key.trim().to_string()),
        model: settings.cleanup_model.trim().to_string(),
        system_prompt,
        wrap_transcript: false,
        disable_thinking: Some(settings.cleanup_disable_thinking),
        temperature: Some(0.0),
        require_complete_output: true,
        max_tokens: None,
    };
    let text = prompts::wrap_cleanup_transcript(&input);
    let handle = crate::services::runtime::spawn(async move {
        let cancel = tokio_util::sync::CancellationToken::new();
        crate::services::cleanup::process_text(&text, &config, &cancel).await
    });
    async move {
        match handle.await {
            Ok(Ok(text)) => Ok(text),
            Ok(Err(err)) => Err(err.to_string()),
            Err(err) => Err(err.to_string()),
        }
    }
}

/// Key events from the native listener used while capturing a hotkey.
#[derive(Clone, Debug)]
pub enum RawKey {
    FnDown,
    FnUp,
    RightModifierDown(String),
    RightModifierUp,
    MouseButtonDown(String),
}

pub fn raw_key_events() -> Option<async_channel::Receiver<RawKey>> {
    let manager = hotkeys()?;
    let raw = manager.subscribe_raw_keys();
    let (tx, rx) = async_channel::unbounded();
    std::thread::spawn(move || {
        use crate::native::hotkeys::RawKeyEvent as E;
        while let Ok(event) = raw.recv_blocking() {
            let mapped = match event {
                E::GlobeDown => RawKey::FnDown,
                E::GlobeUp => RawKey::FnUp,
                E::RightModifierDown(name) => RawKey::RightModifierDown(name),
                E::RightModifierUp(_) => RawKey::RightModifierUp,
                E::MouseButtonDown(name) => RawKey::MouseButtonDown(name),
                _ => continue,
            };
            if tx.send_blocking(mapped).is_err() {
                break;
            }
        }
    });
    Some(rx)
}

pub fn set_hotkey_listening_mode(enabled: bool) {
    if let Some(manager) = hotkeys() {
        manager.set_listening_mode(enabled);
    }
}

pub fn parse_hotkey_list(value: &str) -> Vec<String> {
    crate::text::hotkeys::parse_hotkey_list(value)
}

pub fn serialize_hotkey_list(list: &[String]) -> String {
    crate::text::hotkeys::serialize_hotkey_list(list)
}

pub fn normalize_hotkey(hotkey: &str) -> String {
    crate::text::hotkeys::normalize_hotkey(hotkey)
}

/// The user-facing reason a captured hotkey is rejected, if any.
pub fn validate_hotkey(hotkey: &str) -> Option<String> {
    let message = crate::text::hotkeys::get_validation_message::<&str>(hotkey, &[])?;
    let params: Vec<(&str, &str)> = message
        .params
        .iter()
        .map(|(k, v)| (*k, v.as_str()))
        .collect();
    Some(
        crate::i18n::try_t(message.key)
            .map(|_| crate::i18n::t_with(message.key, &params))
            .unwrap_or(message.default_text),
    )
}

pub fn format_hotkey_label(hotkey: &str) -> String {
    crate::text::hotkeys::format_hotkey_label(Some(hotkey))
}

/// Registers the global Dictation hotkeys; returns the list actually active.
pub fn register_hotkeys(list: &str, cx: &mut gpui_kit::App) -> Result<String, String> {
    let Some(manager) = hotkeys() else {
        return Ok(list.to_string());
    };
    let mode = crate::app_state::AppState::settings(cx).activation_mode;
    manager.register(list, mode).map_err(|err| {
        let message = crate::i18n::t_with(
            err.message_key(),
            &[("hotkey", &format_hotkey_label(&err.hotkey))],
        );
        if err.suggestions.is_empty() {
            message
        } else {
            format!(
                "{message} {}",
                crate::i18n::t_with(
                    "hotkey.errors.trySuggestions",
                    &[("suggestions", &err.suggestions.join(", "))]
                )
            )
        }
    })
}
// ---- Hotkeys ------------------------------------------------------------------------------

static HOTKEYS: once_cell::sync::OnceCell<crate::native::hotkeys::HotkeyManager> =
    once_cell::sync::OnceCell::new();

/// Installs the hotkey manager; call once on the main thread at startup.
pub fn install_hotkeys(manager: crate::native::hotkeys::HotkeyManager) {
    let _ = HOTKEYS.set(manager);
}

pub fn hotkeys() -> Option<&'static crate::native::hotkeys::HotkeyManager> {
    HOTKEYS.get()
}

pub fn register_cancel_hotkey() {
    if let Some(manager) = hotkeys() {
        manager.register_cancel_hotkey();
    }
}

pub fn unregister_cancel_hotkey() {
    if let Some(manager) = hotkeys() {
        manager.unregister_cancel_hotkey();
    }
}

/// Shares Escape with the recording while the copy-recovery panel is open.
pub fn set_recovery_cancel_hotkey(active: bool) {
    use crate::native::hotkeys::CancelHotkeyOwner;
    let Some(manager) = hotkeys() else { return };
    if active {
        if let Err(error) = manager.register_cancel_hotkey_for(CancelHotkeyOwner::CopyRecovery) {
            log::warn!("could not register the recovery Escape shortcut: {error}");
        }
    } else {
        manager.unregister_cancel_hotkey_for(CancelHotkeyOwner::CopyRecovery);
    }
}

pub fn clear_hotkey_start_request() {
    if let Some(manager) = hotkeys() {
        manager.clear_start_request();
    }
}

// ---- Dictation side effects ----------------------------------------------------------------

pub fn pause_media() {
    std::thread::spawn(crate::native::media::pause_media);
}

pub fn resume_media() {
    std::thread::spawn(crate::native::media::resume_media);
}

/// The cpal device name for the configured microphone, `None` for the system default.
pub fn preferred_input_device(settings: &crate::settings::Settings) -> Option<String> {
    crate::native::mic::resolve_preferred_microphone(settings)
        .device
        .map(|d| d.label)
}

/// The Target app at the hotkey press.
pub fn capture_target_pid() -> Option<i32> {
    crate::native::paste::capture_frontmost_pid()
}

pub fn write_clipboard(text: &str) -> bool {
    crate::native::paste::write_clipboard(text)
}

/// Pastes into the Target app off the main thread; resolves to whether it pasted.
pub fn paste_text(
    text: String,
    target_pid: Option<i32>,
    restore_clipboard: bool,
) -> impl std::future::Future<Output = bool> + 'static {
    let (tx, rx) = futures::channel::oneshot::channel();
    std::thread::spawn(move || {
        let outcome = crate::native::paste::paste_text(
            &text,
            crate::native::paste::PasteOptions {
                target_pid,
                restore_clipboard,
                allow_clipboard_fallback: false,
            },
        );
        if let Some(failure) = &outcome.failure {
            log::warn!("automatic paste failed: {failure:?}");
        }
        let _ = tx.send(outcome.pasted);
    });
    async move { rx.await.unwrap_or(false) }
}

/// Watches the target field after an automatic paste and adds learned
/// corrections to the Dictionary (auto-learn). Resolves to the words added.
pub fn monitor_corrections(
    text: String,
    target_pid: Option<i32>,
    cx: &mut gpui_kit::App,
) -> gpui_kit::Task<Vec<String>> {
    let monitor = crate::native::text_monitor::TextEditMonitor::shared();
    let edits = monitor.subscribe();
    monitor.start_monitoring_after_paste(&text, target_pid);
    cx.spawn(async move |cx| {
        let Ok(edit) = edits.recv().await else {
            return Vec::new();
        };
        cx.update(|cx| {
            let state = crate::app_state::AppState::global(cx);
            let (db, dictionary) = {
                let s = state.read(cx);
                (s.db.clone(), s.dictionary.clone())
            };
            let learned = crate::text::correction_learner::extract_corrections(
                &edit.original_text,
                &edit.edited_text,
                &dictionary,
            );
            if learned.is_empty() {
                return Vec::new();
            }
            let empty: [&str; 0] = [];
            if db
                .apply_dictionary_changes(&learned, &empty, crate::db::DictionarySource::Learned)
                .is_err()
            {
                return Vec::new();
            }
            state.update(cx, |s, cx| s.reload_dictionary(cx));
            learned
        })
    })
}

/// Removes words auto-learn just added (the toast's Undo).
pub fn undo_learned_words(words: &[String], cx: &mut gpui_kit::App) {
    let state = crate::app_state::AppState::global(cx);
    let db = state.read(cx).db.clone();
    let empty: [&str; 0] = [];
    let _ = db.apply_dictionary_changes(&empty, words, crate::db::DictionarySource::Learned);
    state.update(cx, |s, cx| s.reload_dictionary(cx));
}
