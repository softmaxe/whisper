//! User preferences, persisted as camelCase JSON in `settings.json`.
//!
//! The Dictionary and Snippets live in SQLite and the cleanup API key lives in
//! the Keychain, so neither is part of this file.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::paths;

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

impl ActivationMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Tap => "tap",
            Self::Push => "push",
        }
    }
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
    pub save_discarded_transcriptions: bool,
    pub audio_cues_enabled: bool,
    pub pause_media_on_dictation: bool,
    pub start_minimized: bool,
    pub show_menu_bar_icon: bool,
    pub panel_start_position: PanelStartPosition,
    pub auto_paste_enabled: bool,
    pub keep_transcription_in_clipboard: bool,
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
            save_discarded_transcriptions: false,
            audio_cues_enabled: true,
            pause_media_on_dictation: false,
            start_minimized: false,
            show_menu_bar_icon: true,
            panel_start_position: PanelStartPosition::BottomRight,
            auto_paste_enabled: true,
            keep_transcription_in_clipboard: false,
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
            .and_then(|raw| serde_json::from_str(&raw).ok())
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

    pub fn is_chinese_ui(&self) -> bool {
        self.ui_language == "zh-CN"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn has_expected_defaults() {
        let s = Settings::default();
        assert!(s.use_cleanup_model);
        assert!(s.cleanup_disable_thinking);
        assert_eq!(s.audio_retention_days, 30);
        assert_eq!(s.transcript_retention_days, 0);
        assert!(s.show_menu_bar_icon);
        assert!(s.auto_paste_enabled);
        assert_eq!(s.activation_mode, ActivationMode::Tap);
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
    }
}
