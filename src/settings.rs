//! User preferences, persisted as camelCase JSON in `settings.json`.
//!
//! The Dictionary and Snippets live in SQLite and the cleanup API key lives in
//! the Keychain, so neither is part of this file.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::paths;

/// Every output mode selects a destination even when History is disabled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum OutputMode {
    #[default]
    PasteAndRestore,
    PasteAndKeep,
    CopyOnly,
}

impl OutputMode {
    pub fn parse(value: &str) -> Self {
        match value {
            "paste-and-keep" => Self::PasteAndKeep,
            "copy-only" => Self::CopyOnly,
            _ => Self::PasteAndRestore,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::PasteAndRestore => "paste-and-restore",
            Self::PasteAndKeep => "paste-and-keep",
            Self::CopyOnly => "copy-only",
        }
    }

    pub fn pastes_automatically(self) -> bool {
        self != Self::CopyOnly
    }

    pub fn restores_clipboard(self) -> bool {
        self == Self::PasteAndRestore
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum MicrophoneSelectionMode {
    #[default]
    Auto,
    BuiltIn,
    Specific,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum PanelStartPosition {
    #[default]
    BottomRight,
    Center,
    BottomLeft,
}

impl PanelStartPosition {
    pub fn parse(value: &str) -> Self {
        match value {
            "center" => Self::Center,
            "bottom-left" => Self::BottomLeft,
            _ => Self::BottomRight,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::BottomRight => "bottom-right",
            Self::Center => "center",
            Self::BottomLeft => "bottom-left",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ThemePreference {
    Light,
    Dark,
    #[serde(rename = "tokyo-night")]
    TokyoNight,
    #[default]
    Auto,
}

/// Tap mode starts Hands-free dictation on a Double tap; Hold mode records
/// while the hotkey is held (stored as "push").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ActivationMode {
    #[default]
    Tap,
    Push,
}

/// When the transcription language is Auto, force a Chinese output script.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum ChineseScriptPreference {
    #[default]
    AsTranscribed,
    Simplified,
    Traditional,
}

impl ChineseScriptPreference {
    pub fn parse(value: &str) -> Self {
        match value {
            "simplified" => Self::Simplified,
            "traditional" => Self::Traditional,
            _ => Self::AsTranscribed,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// "en" or "zh-CN".
    pub ui_language: String,
    /// "auto" or a language code such as "en", "zh-CN".
    pub preferred_language: String,
    pub chinese_script_preference: ChineseScriptPreference,
    pub remote_transcription_url: String,
    pub remote_transcription_model: String,

    pub remove_filler_words: bool,
    pub use_cleanup_model: bool,
    pub cleanup_model: String,
    pub cleanup_remote_url: String,
    pub cleanup_disable_thinking: bool,
    /// Custom cleanup prompt; empty means the shipped default.
    pub custom_prompt_cleanup: String,

    /// Comma-separated hotkey list, e.g. "GLOBE" or "RightCommand,Control+Shift+Space".
    pub dictation_key: String,
    pub activation_mode: ActivationMode,

    pub microphone_selection_mode: MicrophoneSelectionMode,
    pub selected_mic_device_id: String,
    pub selected_mic_device_label: String,

    pub theme: ThemePreference,
    pub audio_retention_days: u32,
    pub transcript_retention_days: u32,
    pub data_retention_enabled: bool,
    pub audio_cues_enabled: bool,
    pub pause_media_on_dictation: bool,
    pub start_minimized: bool,
    pub show_menu_bar_icon: bool,
    pub panel_start_position: PanelStartPosition,
    pub output_mode: OutputMode,
    pub auto_learn_corrections: bool,
    pub sidebar_collapsed: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            ui_language: "en".into(),
            preferred_language: "auto".into(),
            chinese_script_preference: ChineseScriptPreference::AsTranscribed,
            remote_transcription_url: String::new(),
            remote_transcription_model: String::new(),
            remove_filler_words: true,
            use_cleanup_model: true,
            cleanup_model: String::new(),
            cleanup_remote_url: String::new(),
            cleanup_disable_thinking: true,
            custom_prompt_cleanup: String::new(),
            dictation_key: String::new(),
            activation_mode: ActivationMode::Tap,
            microphone_selection_mode: MicrophoneSelectionMode::Auto,
            selected_mic_device_id: String::new(),
            selected_mic_device_label: String::new(),
            theme: ThemePreference::Auto,
            audio_retention_days: 30,
            transcript_retention_days: 0,
            data_retention_enabled: true,
            audio_cues_enabled: true,
            pause_media_on_dictation: false,
            start_minimized: false,
            show_menu_bar_icon: true,
            panel_start_position: PanelStartPosition::BottomRight,
            output_mode: OutputMode::default(),
            auto_learn_corrections: true,
            sidebar_collapsed: false,
        }
    }
}

/// The hotkey used when none is configured: the Globe key.
pub const DEFAULT_HOTKEY: &str = "GLOBE";

impl Settings {
    pub fn load() -> Self {
        Self::load_from(&paths::settings_path())
    }

    pub fn load_from(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|raw| {
                let mut value: serde_json::Value = serde_json::from_str(&raw).ok()?;
                // Missing legacy keys used these defaults. Prefer the new key
                // when both formats are present; save only the new key.
                if let Some(fields) = value.as_object_mut()
                    && !fields.contains_key("outputMode")
                {
                    let paste = fields
                        .get("autoPasteEnabled")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(true);
                    let keep = fields
                        .get("keepTranscriptionInClipboard")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    let mode = match (paste, keep) {
                        (true, false) => OutputMode::PasteAndRestore,
                        (true, true) => OutputMode::PasteAndKeep,
                        (false, _) => OutputMode::CopyOnly,
                    };
                    fields.insert("outputMode".into(), serde_json::to_value(mode).ok()?);
                }
                serde_json::from_value(value).ok()
            })
            .unwrap_or_default()
    }

    pub fn save(&self) -> anyhow::Result<()> {
        self.save_to(&paths::settings_path())
    }

    pub fn save_to(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(tmp, path)?;
        Ok(())
    }

    /// The configured hotkey list, or the default when empty.
    pub fn effective_dictation_key(&self) -> String {
        if self.dictation_key.trim().is_empty() {
            DEFAULT_HOTKEY.to_string()
        } else {
            self.dictation_key.clone()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_output_settings_migrate_on_load_and_save_only_one_key() {
        for (paste, keep, expected) in [
            (true, false, OutputMode::PasteAndRestore),
            (true, true, OutputMode::PasteAndKeep),
            (false, true, OutputMode::CopyOnly),
            (false, false, OutputMode::CopyOnly),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("settings.json");
            std::fs::write(
                &path,
                serde_json::to_vec(&serde_json::json!({
                    "autoPasteEnabled": paste,
                    "keepTranscriptionInClipboard": keep,
                    "theme": "dark",
                    "remoteTranscriptionModel": "test-model",
                    "unknownFutureSetting": true,
                }))
                .unwrap(),
            )
            .unwrap();
            let settings = Settings::load_from(&path);
            assert_eq!(settings.output_mode, expected);
            assert_eq!(settings.theme, ThemePreference::Dark);
            assert_eq!(settings.remote_transcription_model, "test-model");
            settings.save_to(&path).unwrap();
            let saved: serde_json::Value =
                serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            assert_eq!(saved["outputMode"], expected.as_str());
            assert!(saved.get("autoPasteEnabled").is_none());
            assert!(saved.get("keepTranscriptionInClipboard").is_none());
            assert_eq!(Settings::load_from(&path), settings);
        }
    }

    #[test]
    fn output_migration_uses_legacy_defaults_and_prefers_the_new_key() {
        for (raw, expected) in [
            (r#"{}"#, OutputMode::PasteAndRestore),
            (r#"{"autoPasteEnabled":false}"#, OutputMode::CopyOnly),
            (
                r#"{"keepTranscriptionInClipboard":true}"#,
                OutputMode::PasteAndKeep,
            ),
            (
                r#"{"outputMode":"paste-and-keep","autoPasteEnabled":false,"keepTranscriptionInClipboard":false}"#,
                OutputMode::PasteAndKeep,
            ),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("settings.json");
            std::fs::write(&path, raw).unwrap();
            assert_eq!(Settings::load_from(&path).output_mode, expected);
        }
    }

    #[test]
    fn output_modes_have_the_expected_delivery_behavior() {
        for (mode, paste, restore) in [
            (OutputMode::PasteAndRestore, true, true),
            (OutputMode::PasteAndKeep, true, false),
            (OutputMode::CopyOnly, false, false),
        ] {
            assert_eq!(mode.pastes_automatically(), paste);
            assert_eq!(mode.restores_clipboard(), restore);
            assert_eq!(OutputMode::parse(mode.as_str()), mode);
        }
    }

    #[test]
    fn tokyo_night_preference_round_trips() {
        let settings: Settings = serde_json::from_str(r#"{"theme":"tokyo-night"}"#).unwrap();
        assert_eq!(settings.theme, ThemePreference::TokyoNight);
        let saved = serde_json::to_value(settings).unwrap();
        assert_eq!(saved["theme"], "tokyo-night");
    }

    #[test]
    fn round_trips_through_json_with_camel_case_keys() {
        let s = Settings {
            remote_transcription_url: "http://localhost:8000/v1".into(),
            panel_start_position: PanelStartPosition::BottomLeft,
            ..Settings::default()
        };
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("\"remoteTranscriptionUrl\""));
        assert!(json.contains("\"bottom-left\""));
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn missing_fields_take_defaults() {
        let s: Settings = serde_json::from_str(r#"{"theme":"dark"}"#).unwrap();
        assert_eq!(s.theme, ThemePreference::Dark);
        assert_eq!(s.audio_retention_days, 30);
        assert!(s.remove_filler_words);
    }
}
