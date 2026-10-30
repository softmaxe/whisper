//! The UI's view of the platform and services. Each function wraps one call
//! into the library (`crate::native`, `crate::services`, ...) so the views
//! stay free of platform details.

use std::path::PathBuf;

pub use crate::native::autostart::AutoStartState;
use crate::native::permissions::SettingsPane;

pub fn auto_start_state() -> AutoStartState {
    crate::native::autostart::auto_start_state()
}

pub fn set_auto_start(enabled: bool) -> anyhow::Result<()> {
    crate::native::autostart::set_auto_start_enabled(enabled).map_err(anyhow::Error::msg)
}

pub fn open_login_items_settings() {
    open_url(SettingsPane::LoginItems.url());
}

fn open_microphone_privacy_settings() {
    open_url(SettingsPane::Microphone.url());
}

fn open_accessibility_settings() {
    open_url(SettingsPane::Accessibility.url());
}

fn open_url(url: &str) {
    let _ = std::process::Command::new("open").arg(url).spawn();
}

pub fn reveal_in_finder(path: &PathBuf) {
    let _ = std::process::Command::new("open")
        .arg("-R")
        .arg(path)
        .spawn();
}

pub fn list_microphones() -> Vec<crate::native::mic::MicDevice> {
    crate::native::mic::list_input_devices()
}

pub fn system_default_microphone() -> Option<String> {
    crate::native::mic::system_default_microphone()
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

#[derive(Clone, Debug, Default)]
pub struct HotkeyModeInfo {
    pub push_disabled_reason: Option<String>,
}

pub fn hotkey_mode_info(hotkeys_list: &str) -> HotkeyModeInfo {
    let Some(manager) = hotkeys() else {
        return HotkeyModeInfo::default();
    };
    HotkeyModeInfo {
        push_disabled_reason: (!manager.supports_hold(hotkeys_list))
            .then(|| crate::i18n::t("hotkey.errors.holdNeedsReleaseKey")),
    }
}

pub fn list_models(
    base: String,
    api_key: String,
) -> impl std::future::Future<
    Output = Result<(String, Vec<crate::services::cleanup::ModelOption>), String>,
> + 'static {
    let handle = crate::services::runtime::spawn(async move {
        let key = (!api_key.is_empty()).then_some(api_key.as_str());
        crate::services::cleanup::list_models(&base, key).await
    });
    async move {
        match handle.await {
            Ok(Ok(listed)) => Ok(listed),
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
        &chinese_script::resolve_cleanup_language(&settings.preferred_language),
        &dictionary_hints,
        &settings.ui_language,
    );
    let config = crate::services::cleanup::CleanupRequestConfig::dictation(
        settings.cleanup_remote_url.trim(),
        Some(api_key.trim()).filter(|k| !k.is_empty()),
        settings.cleanup_model.trim(),
        &system_prompt,
        settings.cleanup_disable_thinking,
    );
    let handle = crate::services::runtime::spawn(async move {
        let cancel = tokio_util::sync::CancellationToken::new();
        crate::services::cleanup::process_text(&input, &config, &cancel).await
    });
    async move {
        match handle.await {
            Ok(Ok(text)) => Ok(text),
            Ok(Err(err)) => Err(err.to_string()),
            Err(err) => Err(err.to_string()),
        }
    }
}

pub use crate::native::hotkeys::RawKeyEvent;

/// Key events from the native listener, used while capturing a hotkey.
pub fn raw_key_events() -> Option<async_channel::Receiver<RawKeyEvent>> {
    Some(hotkeys()?.subscribe_raw_keys())
}

pub fn set_hotkey_listening_mode(enabled: bool) {
    if let Some(manager) = hotkeys() {
        manager.set_listening_mode(enabled);
    }
}

/// The user-facing reason a captured hotkey is rejected, if any.
pub fn validate_hotkey(hotkey: &crate::text::hotkeys::Hotkey) -> Option<String> {
    let message = hotkey.validation_message()?;
    let params: Vec<(&str, &str)> = message
        .params
        .iter()
        .map(|(k, v)| (*k, v.as_str()))
        .collect();
    Some(crate::i18n::t_with(message.key, &params))
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
            &[(
                "hotkey",
                &crate::text::hotkeys::format_hotkey_list_label(&err.hotkey),
            )],
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

static HOTKEYS: std::sync::OnceLock<crate::native::hotkeys::HotkeyManager> =
    std::sync::OnceLock::new();

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
    crate::native::media::pause_media();
}

pub fn resume_media() {
    crate::native::media::resume_media();
}

/// The cpal device name for the configured microphone, `None` for the system default.
pub fn preferred_input_device(settings: &crate::settings::Settings) -> Option<String> {
    crate::native::mic::resolve_preferred_microphone(settings).map(|d| d.label)
}

/// The Target app at the hotkey press.
pub fn capture_target_pid() -> Option<i32> {
    crate::native::text_monitor::TextEditMonitor::shared().capture_target_pid()
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
        crate::native::text_monitor::TextEditMonitor::shared().stop_monitoring();
        let outcome = crate::native::paste::paste_text(
            &text,
            crate::native::paste::PasteOptions {
                target_pid,
                restore_clipboard,
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
/// corrections to the Dictionary (auto-learn). Resolves to the words added
/// once the session ends.
pub fn monitor_corrections(
    text: String,
    target_pid: Option<i32>,
    cx: &mut gpui_kit::App,
) -> gpui_kit::Task<Vec<String>> {
    let reports = crate::native::text_monitor::TextEditMonitor::shared()
        .start_monitoring_after_paste(&text, target_pid);
    cx.spawn(async move |cx| {
        let Ok(report) = reports.recv().await else {
            return Vec::new();
        };
        cx.update(|cx| {
            crate::app_state::AppState::global(cx)
                .update(cx, |s, cx| s.learn_from_session(report, cx))
        })
    })
}

/// Removes words auto-learn just added (the toast's Undo).
pub fn undo_learned_words(words: &[String], cx: &mut gpui_kit::App) {
    let none: [&str; 0] = [];
    crate::app_state::AppState::global(cx).update(cx, |s, cx| {
        if let Err(err) =
            s.change_dictionary(&none, words, crate::db::DictionarySource::Learned, cx)
        {
            log::warn!("failed to undo learned words: {err}");
        }
    });
}
