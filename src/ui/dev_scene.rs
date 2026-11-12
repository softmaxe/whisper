//! Debug builds only: `WHISPER_DEV_SCENE` opens a fixed UI state at launch so
//! screenshots of every view need no pointer input.
//!
//! Values: `home`, `insights`, `upload`, `dictionary`, `search`,
//! `settings[:general|language|hotkeys|speech|llms|privacy]`, and the pill states
//! `pill:recording`, `pill:processing`, `pill:preparing`, `pill:transition`,
//! `pill:cleanup`, `pill:cleanup-transition`, `pill:flow`, `pill:error`, `pill:fallback`.
//! `WHISPER_DEV_SCROLL_FRACTION=0.5` or `1` captures the middle or bottom of Settings.

use std::time::Duration;

use gpui_kit::*;

use crate::app_state::AppState;
use crate::dictation::{Dictation, DictationNotice, ErrorActions};
use crate::i18n::t;

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
            "hotkeys" | "mic-recording" | "mic-ready" | "mic-error" | "mic-transition" => {
                SettingsSection::Hotkeys
            }
            "speech" => SettingsSection::SpeechToText,
            "llms" => SettingsSection::Llms,
            "privacy" => SettingsSection::PrivacyData,
            _ => SettingsSection::General,
        }),
        "pill" => Scene::Pill(match arg {
            "preparing" => "preparing",
            "transition" => "transition",
            "processing" => "processing",
            "cleanup" => "cleanup",
            "cleanup-transition" => "cleanup-transition",
            "flow" => "flow",
            "error" => "error",
            "no-speech" => "no-speech",
            "fallback" => "fallback",
            _ => "recording",
        }),
        _ => return None,
    })
}

/// How long `pill:recording` pretends the microphone has been live, so the
/// capture shows the pill with its elapsed timer.
pub fn pill_head_start() -> Duration {
    match scene() {
        Some(Scene::Pill("recording")) => Duration::from_secs(14),
        _ => Duration::ZERO,
    }
}

/// Drives the recording pill into the requested state.
pub fn apply_pill(cx: &mut App) {
    let Some(Scene::Pill(state)) = scene() else {
        return;
    };
    match state {
        "flow" => {
            // One whole Dictation on a loop: record, transcribe, polish,
            // skip, deliver. For recording the pill's motion end to end.
            let set = |cx: &mut gpui_kit::AsyncApp, recording, processing, cleaning| {
                cx.update(|cx| {
                    AppState::global(cx).update(cx, |s, cx| {
                        s.dictation.is_preparing = false;
                        s.dictation.is_recording = recording;
                        s.dictation.is_processing = processing;
                        s.dictation.is_cleaning = cleaning;
                        cx.notify();
                    })
                });
            };
            // `WHISPER_DEV_FLOW_MS=transcribe,polish` replays measured
            // server timings instead of the slow defaults.
            let stages: Vec<u64> = std::env::var("WHISPER_DEV_FLOW_MS")
                .ok()
                .map(|v| v.split(',').filter_map(|n| n.trim().parse().ok()).collect())
                .unwrap_or_default();
            let transcribe = stages.first().copied().unwrap_or(1600);
            let polish = stages.get(1).copied().unwrap_or(2400);
            cx.spawn(async move |cx| {
                loop {
                    let executor = cx.background_executor().clone();
                    let wait = |ms| executor.timer(Duration::from_millis(ms));
                    wait(1200).await;
                    set(cx, true, false, false);
                    wait(2200).await;
                    set(cx, false, true, false);
                    wait(transcribe).await;
                    set(cx, false, true, true);
                    wait(polish).await;
                    set(cx, false, true, false);
                    wait(if stages.is_empty() { 250 } else { 20 }).await;
                    set(cx, false, false, false);
                    cx.update(|cx| {
                        Dictation::global(cx)
                            .update(cx, |_, cx| cx.emit(DictationNotice::Delivered))
                    });
                    wait(1200).await;
                }
            })
            .detach();
        }
        "cleanup-transition" => {
            AppState::global(cx).update(cx, |s, cx| {
                s.dictation.is_processing = true;
                cx.notify();
            });
            cx.spawn(async |cx| {
                loop {
                    cx.background_executor().timer(Duration::from_secs(2)).await;
                    cx.update(|cx| {
                        AppState::global(cx).update(cx, |s, cx| {
                            s.dictation.is_cleaning = !s.dictation.is_cleaning;
                            cx.notify();
                        });
                    });
                }
            })
            .detach();
        }
        "preparing" | "transition" => {
            AppState::global(cx).update(cx, |s, cx| {
                s.dictation.is_preparing = true;
                cx.notify();
            });
            if state == "transition" {
                cx.spawn(async |cx| {
                    loop {
                        cx.background_executor()
                            .timer(Duration::from_millis(2300))
                            .await;
                        cx.update(|cx| {
                            AppState::global(cx).update(cx, |s, cx| {
                                s.dictation.is_preparing = false;
                                s.dictation.is_recording = true;
                                cx.notify();
                            })
                        });
                        cx.background_executor().timer(Duration::from_secs(6)).await;
                        cx.update(|cx| {
                            AppState::global(cx).update(cx, |s, cx| {
                                s.dictation = Default::default();
                                cx.notify();
                            })
                        });
                        cx.background_executor().timer(Duration::from_secs(1)).await;
                        cx.update(|cx| {
                            AppState::global(cx).update(cx, |s, cx| {
                                s.dictation.is_preparing = true;
                                cx.notify();
                            })
                        });
                    }
                })
                .detach();
            }
        }
        "recording" | "processing" | "cleanup" => {
            AppState::global(cx).update(cx, |s, cx| {
                s.dictation.is_recording = state == "recording";
                s.dictation.is_processing = state == "processing" || state == "cleanup";
                s.dictation.is_cleaning = state == "cleanup";
                cx.notify();
            });
        }
        "no-speech" => Dictation::global(cx).update(cx, |_, cx| {
            cx.emit(DictationNotice::Error {
                title: t("dictation.noAudio.title"),
                description: t("dictation.noAudio.description"),
                actions: ErrorActions::for_transcription_failure(Some(
                    crate::audio::vad::NO_SPEECH_CODE,
                )),
                kept: Some(Duration::from_secs(10)),
            });
        }),
        "error" => Dictation::global(cx).update(cx, |_, cx| {
            cx.emit(DictationNotice::Error {
                title: t("dictation.errorTitles.transcriptionTimeout"),
                description: t("dictation.errorDescriptions.transcriptionTimeout"),
                actions: ErrorActions::for_transcription_failure(Some(
                    crate::services::transcription::REQUEST_TIMEOUT_CODE,
                )),
                kept: Some(Duration::from_secs(42)),
            })
        }),
        _ => Dictation::global(cx).update(cx, |_, cx| {
            cx.emit(DictationNotice::DeliveryFallback {
                text: "Let’s move the design review to Thursday at 3 PM.".into(),
                copied: true,
            })
        }),
    }
}
