//! Debug builds only: `WHISPER_DEV_SCENE` opens a fixed UI state at launch so
//! screenshots of every view need no pointer input.
//!
//! Values: `home`, `insights`, `upload`, `dictionary`, `search`,
//! `settings[:general|hotkeys|speech|llms|privacy]`, and the pill states
//! `pill:recording`, `pill:processing`, `pill:error`, `pill:fallback`.

use gpui_kit::*;

use crate::app_state::AppState;
use crate::dictation::{Dictation, DictationNotice};

use super::control_panel::PanelView;
use super::settings_modal::SettingsSection;

pub enum Scene {
    View(PanelView),
    Search,
    Settings(SettingsSection),
    Pill(&'static str),
}

pub fn scene() -> Option<Scene> {
    let value = std::env::var("WHISPER_DEV_SCENE").ok()?;
    let (head, arg) = value.split_once(':').unwrap_or((value.as_str(), ""));
    Some(match head {
        "home" => Scene::View(PanelView::Home),
        "insights" => Scene::View(PanelView::Insights),
        "upload" => Scene::View(PanelView::Upload),
        "dictionary" => Scene::View(PanelView::Dictionary),
        "search" => Scene::Search,
        "settings" => Scene::Settings(match arg {
            "hotkeys" => SettingsSection::Hotkeys,
            "speech" => SettingsSection::SpeechToText,
            "llms" => SettingsSection::Llms,
            "privacy" => SettingsSection::PrivacyData,
            _ => SettingsSection::General,
        }),
        "pill" => Scene::Pill(match arg {
            "processing" => "processing",
            "error" => "error",
            "fallback" => "fallback",
            _ => "recording",
        }),
        _ => return None,
    })
}

/// Drives the recording pill into the requested state.
pub fn apply_pill(cx: &mut App) {
    let Some(Scene::Pill(state)) = scene() else {
        return;
    };
    match state {
        "recording" | "processing" => {
            AppState::global(cx).update(cx, |s, cx| {
                s.dictation.is_recording = state == "recording";
                s.dictation.is_processing = state == "processing";
                cx.notify();
            });
        }
        "error" => Dictation::global(cx).update(cx, |_, cx| {
            cx.emit(DictationNotice::Error {
                title: "Transcription failed".into(),
                description: "The speech server did not answer within 30 seconds.".into(),
            })
        }),
        _ => Dictation::global(cx).update(cx, |_, cx| {
            cx.emit(DictationNotice::DeliveryFallback {
                text: "Let's move the design review to Thursday at 3 PM.".into(),
                copied: true,
            })
        }),
    }
}
