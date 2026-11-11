//! Upload: transcribe audio and video files one at a time or as a batch.
//!
//! The view entity outlives navigation, so a running batch keeps draining
//! while the user looks at other pages.

use std::f32::consts::TAU;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use tokio_util::sync::CancellationToken;

use crate::i18n::{t, t_with};
use crate::services::transcription::CUSTOM_ENDPOINT_INVALID_CODE;
use crate::services::upload::{
    FileTranscriptionRequest, UploadError, is_supported_upload_file, transcribe_audio_file,
};

use crate::app_state::AppState;

use super::components::{
    Button, ButtonSize, ButtonVariant, drop, empty_state, f32_tx, icon, page_container, spinner,
    surface_card,
};
use super::format::format_elapsed;
use super::history_view::copy_with_toast;
use super::settings_modal::SettingsSection;
use super::theme::{
    DUR_SHEET, FONT_VOICE, LINE_READING, LINE_SMALL, LINE_TITLE, Palette, PaletteExt, RADIUS_LG,
    RADIUS_XL, READING_MAX_WIDTH, TEXT_BODY, TEXT_READING, TEXT_SMALL, TEXT_TITLE, ease_out,
    motion, tabular_nums,
};

pub enum UploadEvent {
    OpenHistory,
    RevealHistory(i64),
    OpenSettings(SettingsSection),
}

#[derive(Clone)]
struct SelectedFile {
    name: String,
    path: PathBuf,
    size: String,
}

#[derive(Clone, PartialEq)]
enum Stage {
    Idle,
    Selected,
    Transcribing,
    Complete {
        text: String,
        transcription_id: Option<i64>,
        save_error: Option<String>,
    },
    Error(String),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ItemStatus {
    Queued,
    Transcribing,
    Done,
    Error,
    /// Stopped by the user before it ran: neither done nor failed.
    Cancelled,
}

#[derive(Clone)]
struct QueueItem {
    id: String,
    name: String,
    path: PathBuf,
    status: ItemStatus,
    error: Option<String>,
    text: Option<String>,
    transcription_id: Option<i64>,
    /// When its request went out, for the elapsed time beside it.
    started: Option<Instant>,
}

pub struct UploadView {
    stage: Stage,
    file: Option<SelectedFile>,
    run_id: u64,
    active_request: Option<CancellationToken>,
    skipped_notice: Option<String>,
    queue: Vec<QueueItem>,
    batch_processing: bool,
    batch_run: u64,
    batch_request: Option<CancellationToken>,
    /// When the single file's request went out.
    started: Option<Instant>,
    /// Whether the once-a-second repaint for elapsed times is running.
    ticking: bool,
}

impl EventEmitter<UploadEvent> for UploadView {}

fn format_file_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024. * 1024.))
    }
}

fn file_name(path: &std::path::Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "audio".into())
}

fn error_text(err: &UploadError) -> String {
    if err.code.as_deref() == Some(CUSTOM_ENDPOINT_INVALID_CODE) {
        return t("upload.customEndpointInvalid");
    }
    if let Some(key) = &err.message_key
        && let Some(text) = crate::i18n::try_t(key)
    {
        return text;
    }
    if err.message.is_empty() {
        t("upload.transcriptionFailed")
    } else {
        err.message.clone()
    }
}

fn build_request(path: PathBuf, cx: &App) -> FileTranscriptionRequest {
    let s = AppState::settings(cx);
    FileTranscriptionRequest {
        file_path: path,
        preferred_language: s.preferred_language.clone(),
        remote_transcription_url: s.remote_transcription_url.clone(),
        remote_transcription_model: s.remote_transcription_model.clone(),
    }
}

/// Saves an upload's raw transcript to History when retention is on.
fn save_upload(text: String, cx: &mut App) -> Task<anyhow::Result<Option<i64>>> {
    AppState::global(cx).update(cx, |s, cx| {
        s.record_history(cx, move |history, settings| {
            history.record_upload(&text, settings)
        })
    })
}

fn run_transcription(
    request: FileTranscriptionRequest,
    cancel: CancellationToken,
) -> impl std::future::Future<Output = Result<String, UploadError>> {
    let handle = crate::services::runtime::spawn(transcribe_audio_file(request, cancel));
    async move {
        handle.await.unwrap_or_else(|err| {
            Err(UploadError {
                message: err.to_string(),
                code: None,
                message_key: None,
            })
        })
    }
}

impl UploadView {
    pub fn new(_cx: &mut Context<Self>) -> Self {
        Self {
            stage: Stage::Idle,
            file: None,
            run_id: 0,
            active_request: None,
            skipped_notice: None,
            queue: Vec::new(),
            batch_processing: false,
            batch_run: 0,
            batch_request: None,
            started: None,
            ticking: false,
        }
    }

    fn transcribing_now(&self) -> bool {
        self.stage == Stage::Transcribing
            || self
                .queue
                .iter()
                .any(|i| i.status == ItemStatus::Transcribing)
    }

    /// Repaints once a second while anything transcribes, so elapsed times
    /// count up; stops on its own when nothing does.
    fn start_ticker(&mut self, cx: &mut Context<Self>) {
        if self.ticking {
            return;
        }
        self.ticking = true;
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                let running = this
                    .update(cx, |this, cx| {
                        let running = this.transcribing_now();
                        if running {
                            cx.notify();
                        } else {
                            this.ticking = false;
                        }
                        running
                    })
                    .unwrap_or(false);
                if !running {
                    break;
                }
            }
        })
        .detach();
    }

    fn provider_ready(cx: &App) -> bool {
        !AppState::settings(cx)
            .remote_transcription_url
            .trim()
            .is_empty()
    }

    fn reset(&mut self, cx: &mut Context<Self>) {
        self.stage = Stage::Idle;
        self.file = None;
        self.skipped_notice = None;
        cx.notify();
    }

    fn accept_paths(&mut self, paths: Vec<PathBuf>, from_drop: bool, cx: &mut Context<Self>) {
        let mut valid = Vec::new();
        let mut skipped = Vec::new();
        for path in paths {
            let name = file_name(&path);
            if from_drop && !is_supported_upload_file(&name) {
                skipped.push(name);
                continue;
            }
            valid.push(path);
        }
        if from_drop {
            self.skipped_notice = (!skipped.is_empty())
                .then(|| t_with("upload.unsupportedFiles", &[("names", &skipped.join(", "))]));
        }
        if valid.is_empty() {
            cx.notify();
            return;
        }
        let has_queue = !self.queue.is_empty();
        if valid.len() == 1 && !self.batch_processing && !has_queue {
            let path = valid.remove(0);
            let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            self.file = Some(SelectedFile {
                name: file_name(&path),
                size: if size > 0 {
                    format_file_size(size)
                } else {
                    String::new()
                },
                path,
            });
            self.stage = Stage::Selected;
        } else {
            for path in valid {
                self.queue.push(QueueItem {
                    id: uuid::Uuid::new_v4().to_string(),
                    name: file_name(&path),
                    path,
                    status: ItemStatus::Queued,
                    error: None,
                    text: None,
                    transcription_id: None,
                    started: None,
                });
            }
        }
        cx.notify();
    }

    fn browse(&mut self, cx: &mut Context<Self>) {
        // An async panel keeps GPUI's run loop free while the dialog is open.
        cx.spawn(async move |this, cx| {
            if let Some(paths) = crate::services::upload::select_audio_files(true).await {
                let _ = this.update(cx, |this, cx| this.accept_paths(paths, false, cx));
            }
        })
        .detach();
    }

    fn transcribe(&mut self, cx: &mut Context<Self>) {
        let Some(file) = self.file.clone() else {
            return;
        };
        if self.batch_processing {
            return;
        }
        self.run_id += 1;
        let run = self.run_id;
        let cancel = CancellationToken::new();
        self.active_request = Some(cancel.clone());
        self.stage = Stage::Transcribing;
        self.started = Some(Instant::now());
        self.start_ticker(cx);
        cx.notify();
        let request = build_request(file.path.clone(), cx);
        let task = run_transcription(request, cancel);
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let save = this
                .update(cx, |this, cx| {
                    if run != this.run_id {
                        return None;
                    }
                    match &result {
                        Ok(text) if !text.trim().is_empty() => Some(save_upload(text.clone(), cx)),
                        _ => None,
                    }
                })
                .ok()
                .flatten();
            let saved = match save {
                Some(save) => Some(save.await),
                None => None,
            };
            let _ = this.update(cx, |this, cx| {
                // Every new run or cancel bumps `run_id`, so a stale run no
                // longer owns `active_request`.
                if run != this.run_id {
                    return;
                }
                this.active_request = None;
                this.stage = match result {
                    Ok(text) if !text.trim().is_empty() => match saved {
                        Some(Ok(id)) => Stage::Complete {
                            text,
                            transcription_id: id,
                            save_error: None,
                        },
                        _ => Stage::Complete {
                            text,
                            transcription_id: None,
                            save_error: Some(t("upload.saveFailed")),
                        },
                    },
                    Ok(_) => Stage::Error(t("upload.transcriptionFailed")),
                    Err(err) => Stage::Error(error_text(&err)),
                };
                cx.notify();
            });
        })
        .detach();
    }

    fn cancel_transcription(&mut self, cx: &mut Context<Self>) {
        if let Some(cancel) = self.active_request.take() {
            cancel.cancel();
        }
        self.run_id += 1;
        self.reset(cx);
    }

    // ---- Batch -----------------------------------------------------------------------------

    fn process_queue(&mut self, cx: &mut Context<Self>) {
        if self.batch_processing || self.stage == Stage::Transcribing {
            return;
        }
        self.skipped_notice = None;
        // Files a cancel stopped go back in line with the rest.
        for item in &mut self.queue {
            if item.status == ItemStatus::Cancelled {
                item.status = ItemStatus::Queued;
            }
        }
        self.batch_processing = true;
        self.batch_run += 1;
        let run = self.batch_run;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let mut processed = std::collections::HashSet::new();
            loop {
                let next = this
                    .update(cx, |this, cx| {
                        if run != this.batch_run {
                            return None;
                        }
                        let item = this.queue.iter_mut().find(|i| {
                            i.status == ItemStatus::Queued && !processed.contains(&i.id)
                        })?;
                        item.status = ItemStatus::Transcribing;
                        item.started = Some(Instant::now());
                        let request = build_request(item.path.clone(), cx);
                        let id = item.id.clone();
                        let cancel = CancellationToken::new();
                        this.batch_request = Some(cancel.clone());
                        this.start_ticker(cx);
                        cx.notify();
                        Some((id, request, cancel))
                    })
                    .ok()
                    .flatten();
                let Some((id, request, cancel)) = next else {
                    break;
                };
                processed.insert(id.clone());
                let result = run_transcription(request, cancel).await;
                let save = this
                    .update(cx, |this, cx| {
                        if run != this.batch_run {
                            return None;
                        }
                        match &result {
                            Ok(text) if !text.trim().is_empty() => {
                                Some(save_upload(text.clone(), cx))
                            }
                            _ => None,
                        }
                    })
                    .ok()
                    .flatten();
                let outcome = match save {
                    Some(save) => Some(save.await),
                    None => None,
                };
                let still_current = this
                    .update(cx, |this, cx| {
                        if run != this.batch_run {
                            return false;
                        }
                        this.batch_request = None;
                        if let Some(item) = this.queue.iter_mut().find(|i| i.id == id) {
                            match (&result, outcome) {
                                (Ok(text), Some(Ok(saved))) => {
                                    item.text = Some(text.clone());
                                    item.status = ItemStatus::Done;
                                    item.transcription_id = saved;
                                }
                                (Ok(text), Some(Err(_))) => {
                                    item.text = Some(text.clone());
                                    item.status = ItemStatus::Error;
                                    item.error = Some(t("upload.saveFailed"));
                                }
                                (Ok(_), None) => {
                                    item.status = ItemStatus::Error;
                                    item.error = Some(t("upload.batchTranscriptionFailed"));
                                }
                                (Err(err), _) => {
                                    item.status = ItemStatus::Error;
                                    item.error = Some(error_text(err));
                                }
                            }
                        }
                        cx.notify();
                        true
                    })
                    .unwrap_or(false);
                if !still_current {
                    return;
                }
            }
            let _ = this.update(cx, |this, cx| {
                if run == this.batch_run {
                    this.batch_processing = false;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn cancel_batch(&mut self, cx: &mut Context<Self>) {
        self.batch_run += 1;
        if let Some(cancel) = self.batch_request.take() {
            cancel.cancel();
        }
        self.batch_processing = false;
        // The user's own stop is not a failure, so these files read as
        // cancelled and stay out of the failed count.
        for item in &mut self.queue {
            if matches!(item.status, ItemStatus::Queued | ItemStatus::Transcribing) {
                item.status = ItemStatus::Cancelled;
            }
        }
        cx.notify();
    }

    fn clear_queue(&mut self, cx: &mut Context<Self>) {
        self.cancel_batch(cx);
        self.queue.clear();
        self.skipped_notice = None;
        cx.notify();
    }

    // ---- Rendering -------------------------------------------------------------------------

    fn render_stage_panel(&mut self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let p = cx.palette();
        let ready = Self::provider_ready(cx);
        let stage = self.stage.clone();
        let accepts_drop = ready && stage != Stage::Transcribing;
        let browsable = ready && stage == Stage::Idle;
        let complete = matches!(stage, Stage::Complete { .. });
        let content = match stage.clone() {
            Stage::Idle if !ready => self.render_no_provider(window, cx),
            Stage::Idle => self.render_idle(cx),
            Stage::Selected => self.render_selected(cx),
            Stage::Transcribing => self.render_transcribing(cx),
            Stage::Complete {
                text,
                transcription_id,
                save_error,
            } => self.render_complete(text, transcription_id, save_error, cx),
            Stage::Error(error) => self.render_error(error, cx),
        };
        let notice = self.skipped_notice.clone();

        div()
            .id("upload-stage")
            .group(STAGE_GROUP)
            .relative()
            .flex()
            .flex_col()
            .rounded(px(RADIUS_XL))
            .border_1()
            .bg(p.card)
            // A queue below takes over as the page's focus, so the target shrinks.
            .when(!complete, |d| {
                d.min_h(px(if self.queue.is_empty() {
                    stage_height(window)
                } else {
                    STAGE_COMPACT_HEIGHT
                }))
            })
            // The dashed edge marks an empty drop target; other stages are
            // ordinary raised cards. The edge clears 3:1 against the canvas
            // so the target's bounds read as a control, not decoration.
            .when(browsable, |d| {
                d.border_dashed()
                    .border_color(p.foreground.opacity(if p.dark { 0.4 } else { 0.5 }))
                    .cursor_pointer()
                    // A faint wash joins the stronger edge, so the whole
                    // target answers the pointer, not just its outline.
                    .hover(|s| {
                        s.border_color(p.foreground.opacity(if p.dark { 0.55 } else { 0.62 }))
                            .bg(p
                                .card
                                .blend(p.hover_wash(if p.dark { 0.03 } else { 0.025 })))
                    })
                    // A mouse-only shortcut across the whole target; the Choose
                    // Files button inside is the keyboard and VoiceOver path.
                    .on_click(cx.listener(|this, _, _, cx| this.browse(cx)))
            })
            .when(!browsable, |d| {
                d.border_color(panel_border(&p))
                    .shadow(vec![drop(0., 1., 2., 0.04)])
            })
            .when(accepts_drop, |d| {
                d.drag_over::<ExternalPaths>(move |s, _, _, _| {
                    s.border_color(p.foreground.opacity(0.75))
                })
                .on_drop(cx.listener(|this, paths: &ExternalPaths, _, cx| {
                    this.accept_paths(paths.paths().to_vec(), true, cx);
                }))
            })
            .child(
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .child(content)
                    .with_animation(
                        SharedString::from(format!("upload-stage-{}", stage_key(&stage))),
                        motion(DUR_SHEET, cx).with_easing(ease_out),
                        |d, t| d.opacity(t),
                    ),
            )
            .when_some(notice, |d, notice| {
                d.child(
                    div()
                        .flex()
                        .items_start()
                        .justify_center()
                        .gap(px(6.))
                        .px(px(24.))
                        .pb(px(16.))
                        .child(
                            div()
                                .mt(px(3.))
                                .child(icon("alert-triangle", 12., p.amber_icon)),
                        )
                        .child(
                            div()
                                .min_w_0()
                                .text_size(px(TEXT_SMALL))
                                .line_height(px(LINE_SMALL))
                                .text_color(p.amber_text)
                                .child(notice),
                        ),
                )
            })
            .when(accepts_drop, |d| d.child(drop_overlay(&p)))
            .into_any_element()
    }

    fn render_idle(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let p = cx.palette();
        stage_body()
            .child(glyph_tile("upload", p.muted_foreground, &p))
            .child(stage_title(t("upload.idleTitle"), &p).mt(px(16.)))
            .child(stage_text(t("upload.idleHint"), &p).mt(px(6.)))
            .child(
                div().mt(px(20.)).flex().child(
                    // The page's one primary action while nothing is picked.
                    Button::new("upload-browse")
                        .label(t("upload.chooseFiles"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            // The panel behind also browses on click.
                            cx.stop_propagation();
                            this.browse(cx);
                        })),
                ),
            )
            .child(stage_hint(t("upload.dropHere"), &p).mt(px(10.)))
            .into_any_element()
    }

    fn render_no_provider(&mut self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let p = cx.palette();
        super::components::empty_state_placement(
            px(stage_height(window)),
            div()
                .flex()
                .flex_col()
                .items_center()
                .child(empty_state(
                    "settings",
                    t("upload.noProviderTitle"),
                    t("upload.noProviderHint"),
                    &p,
                ))
                .child(
                    div().mt(px(20.)).flex().child(
                        Button::new("upload-no-provider")
                            .label(t("upload.noProviderAction"))
                            .on_click(cx.listener(|_, _, _, cx| {
                                cx.emit(UploadEvent::OpenSettings(SettingsSection::SpeechToText))
                            })),
                    ),
                ),
        )
        .px(px(32.))
        .into_any_element()
    }

    fn render_selected(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let p = cx.palette();
        let file = self.file.clone().expect("selected file");
        let disabled = self.batch_processing || !Self::provider_ready(cx);
        stage_body()
            .child(glyph_tile("file-audio", p.muted_foreground, &p))
            .child(stage_title(file.name.clone(), &p).mt(px(16.)).truncate())
            .when(!file.size.is_empty(), |d| {
                d.child(stage_hint(file.size.clone(), &p).mt(px(2.)))
            })
            .child(
                div()
                    .mt(px(20.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        Button::new("upload-transcribe")
                            .label(t("upload.transcribe"))
                            .disabled(disabled)
                            .on_click(cx.listener(|this, _, _, cx| this.transcribe(cx))),
                    )
                    .child(
                        Button::new("upload-cancel")
                            .label(t("upload.cancel"))
                            .variant(ButtonVariant::Ghost)
                            .on_click(cx.listener(|this, _, _, cx| this.reset(cx))),
                    ),
            )
            .into_any_element()
    }

    fn render_transcribing(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let p = cx.palette();
        let name = self.file.as_ref().map(|f| f.name.clone());
        // A running clock tells a long file from a stalled request.
        let elapsed = format_elapsed(self.started.map_or(Duration::ZERO, |s| s.elapsed()));
        stage_body()
            .child(live_waveform(&p, cx))
            .child(
                stage_title(
                    t_with("upload.transcribingElapsed", &[("time", &elapsed)]),
                    &p,
                )
                .mt(px(16.))
                .font_features(tabular_nums()),
            )
            .when_some(name, |d, name| {
                d.child(stage_text(name, &p).mt(px(2.)).truncate())
            })
            .child(stage_hint(t("upload.completionHint"), &p).mt(px(6.)))
            .child(
                div().mt(px(20.)).flex().child(
                    Button::new("upload-cancel-transcription")
                        .label(t("upload.cancelTranscription"))
                        .variant(ButtonVariant::Outline)
                        .on_click(cx.listener(|this, _, _, cx| this.cancel_transcription(cx))),
                ),
            )
            .into_any_element()
    }

    fn render_complete(
        &mut self,
        text: String,
        transcription_id: Option<i64>,
        save_error: Option<String>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = cx.palette();
        let file_name = self.file.as_ref().map(|f| f.name.clone());
        let copy_text = text.clone();
        let saved = transcription_id.filter(|_| save_error.is_none());
        let status = match (&save_error, transcription_id) {
            (Some(err), _) => err.clone(),
            (None, Some(_)) => t("upload.savedToHistory"),
            (None, None) => t("controlPanel.history.dataRetentionDisabled"),
        };
        div()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .pl(px(20.))
                    .pr(px(12.))
                    .py(px(10.))
                    .border_b_1()
                    .border_color(panel_border(&p))
                    .child(icon("check", 14., p.success))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                div()
                                    .flex_none()
                                    .text_size(px(TEXT_BODY))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(p.foreground)
                                    .child(t("upload.transcriptionComplete")),
                            )
                            .when_some(file_name, |d, name| {
                                d.child(
                                    div()
                                        .min_w_0()
                                        .truncate()
                                        .text_size(px(TEXT_SMALL))
                                        .text_color(p.subtle_foreground)
                                        .child(name),
                                )
                            }),
                    )
                    .child(
                        Button::new("upload-copy-full")
                            .label(t("upload.copyFullText"))
                            .icon("copy", 13.)
                            .variant(ButtonVariant::Outline)
                            .size(ButtonSize::Sm)
                            .on_click(move |_, _, cx| copy_with_toast(copy_text.clone(), cx)),
                    ),
            )
            .child(
                div().px(px(24.)).py(px(20.)).child(
                    div()
                        .max_w(px(READING_MAX_WIDTH))
                        .font_family(FONT_VOICE)
                        .text_size(px(TEXT_READING))
                        .line_height(px(LINE_READING))
                        .text_color(p.foreground)
                        .child(text),
                ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .pl(px(20.))
                    .pr(px(12.))
                    .py(px(8.))
                    .border_t_1()
                    .border_color(panel_border(&p))
                    .when(save_error.is_some(), |d| {
                        d.child(icon("alert-circle", 13., p.destructive))
                    })
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(TEXT_SMALL))
                            .text_color(if save_error.is_some() {
                                p.muted_foreground
                            } else {
                                p.subtle_foreground
                            })
                            .child(status),
                    )
                    .when_some(saved, |d, id| {
                        d.child(
                            Button::new("upload-view-history")
                                .label(t("upload.viewInHistory"))
                                .variant(ButtonVariant::Ghost)
                                .size(ButtonSize::Sm)
                                .on_click(cx.listener(move |_, _, _, cx| {
                                    cx.emit(UploadEvent::RevealHistory(id))
                                })),
                        )
                    })
                    .child(
                        Button::new("upload-another")
                            .label(t("upload.uploadAnother"))
                            .variant(ButtonVariant::Ghost)
                            .size(ButtonSize::Sm)
                            .on_click(cx.listener(|this, _, _, cx| this.reset(cx))),
                    ),
            )
            .into_any_element()
    }

    fn render_error(&mut self, error: String, cx: &mut Context<Self>) -> AnyElement {
        let p = cx.palette();
        let has_file = self.file.is_some();
        let name = self.file.as_ref().map(|f| f.name.clone());
        let title = t("upload.transcriptionFailed");
        // Generic failures carry no detail beyond the title.
        let detail = (error != title).then_some(error);
        stage_body()
            .child(
                glyph_tile("alert-circle", p.destructive, &p)
                    .bg(p.destructive.opacity(if p.dark { 0.12 } else { 0.06 }))
                    .border_color(p.destructive.opacity(0.22)),
            )
            .child(stage_title(title, &p).mt(px(16.)))
            .when_some(name, |d, name| {
                d.child(stage_hint(name, &p).mt(px(2.)).w_full().truncate())
            })
            .when_some(detail, |d, detail| {
                d.child(stage_text(detail, &p).mt(px(8.)))
            })
            .child(
                div()
                    .mt(px(20.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .when(has_file, |d| {
                        d.child(
                            Button::new("upload-retry")
                                .label(t("upload.retry"))
                                .on_click(cx.listener(|this, _, _, cx| this.transcribe(cx))),
                        )
                    })
                    .child(
                        Button::new("upload-start-over")
                            .label(t("upload.startOver"))
                            .variant(if has_file {
                                ButtonVariant::Ghost
                            } else {
                                ButtonVariant::Outline
                            })
                            .on_click(cx.listener(|this, _, _, cx| this.reset(cx))),
                    ),
            )
            .into_any_element()
    }

    /// One line naming where files go, with a shortcut to change it.
    /// The transcription settings in use and, while nothing else answers it,
    /// where finished transcripts go: one card, one left edge.
    fn render_settings(&mut self, show_destination: bool, cx: &mut Context<Self>) -> AnyElement {
        let p = cx.palette();
        let s = AppState::settings(cx);
        let url = s.remote_transcription_url.trim();
        let server = if url.is_empty() {
            "—".to_string()
        } else {
            super::settings_modal::endpoint_host(url)
        };
        let model = s.remote_transcription_model.trim().to_string();
        // Name the language the way the Speech to text select does: Auto-detect
        // in the UI language, every other language by its own name.
        let language = if s.preferred_language == "auto" {
            t("settings.language.autoDetect")
        } else {
            crate::text::language::languages()
                .iter()
                .find(|l| l.code == s.preferred_language)
                .map(|l| l.native.clone())
                .unwrap_or_else(|| s.preferred_language.clone())
        };
        // One label style and one value weight: the values are context for
        // the drop target, not headlines of their own.
        let pair = |label: String, value: Div| {
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .min_w_0()
                .child(
                    div()
                        .flex_none()
                        .text_size(px(TEXT_SMALL))
                        .text_color(p.muted_foreground)
                        .child(label),
                )
                .child(value.min_w_0().truncate().text_color(p.foreground))
        };
        // A summary, not an editor: all three values share the UI face, so
        // none outweighs its neighbours. Settings keeps mono where they are
        // edited.
        let code = |text: String| {
            if text.is_empty() {
                div().text_size(px(TEXT_SMALL)).child("—")
            } else {
                div()
                    .text_size(px(TEXT_SMALL))
                    .font_features(tabular_nums())
                    .child(text)
            }
        };
        let settings = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_x(px(20.))
            .gap_y(px(4.))
            .min_h(px(44.))
            .pl(px(16.))
            .pr(px(8.))
            .py(px(7.))
            // What people choose comes first; the server is plumbing.
            .child(pair(t("upload.model"), code(model)))
            .child(pair(
                t("upload.language"),
                div().text_size(px(TEXT_SMALL)).child(language),
            ))
            .child(pair(t("upload.server"), code(server)))
            .child(div().flex_1())
            .child(
                // The glyph marks it as an action, not one more value in the
                // line; the label names the settings pane it opens.
                Button::new("upload-change-settings")
                    .label(t("upload.transcriptionSettings"))
                    .icon("settings", 13.)
                    .variant(ButtonVariant::Ghost)
                    .size(ButtonSize::Sm)
                    .on_click(cx.listener(|_, _, _, cx| {
                        cx.emit(UploadEvent::OpenSettings(SettingsSection::SpeechToText))
                    })),
            );
        surface_card(&p)
            .flex()
            .flex_col()
            .child(settings)
            .when(show_destination, |d| {
                d.child(
                    self.render_destination(cx)
                        .border_t_1()
                        .border_color(p.border),
                )
            })
            .into_any_element()
    }

    fn render_queue(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let p = cx.palette();
        let total = self.queue.len();
        let count = |status: ItemStatus| self.queue.iter().filter(|i| i.status == status).count();
        let completed = count(ItemStatus::Done);
        let failed = count(ItemStatus::Error);
        let cancelled = count(ItemStatus::Cancelled);
        let queued = count(ItemStatus::Queued);
        let finished = completed + failed + cancelled;
        let all_done = finished == total;
        let progress = f32_tx(
            "upload-batch-progress",
            finished as f32 / total.max(1) as f32,
            500,
            ease_out,
            window,
            cx,
        );
        let show_progress = self.batch_processing || finished > 0;
        let unsaved = self
            .queue
            .iter()
            .any(|i| i.status == ItemStatus::Done && i.transcription_id.is_none());
        // While a batch runs the header cancels it; otherwise it clears the list.
        let cancels = self.batch_processing && !all_done;
        // Cancelled files can be started again with the ones still queued.
        let startable = queued + cancelled;
        let can_start = !self.batch_processing && startable > 0;
        let start_disabled = !Self::provider_ready(cx) || self.stage == Stage::Transcribing;

        let header = div()
            .flex()
            .items_center()
            .gap(px(8.))
            .min_h(px(44.))
            .pl(px(16.))
            .pr(px(8.))
            .py(px(6.))
            .child(
                div()
                    .flex_none()
                    .text_size(px(TEXT_BODY))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(p.foreground)
                    .child(t("upload.queueTitle")),
            )
            .child(
                div()
                    .flex_none()
                    .text_size(px(TEXT_SMALL))
                    .text_color(p.subtle_foreground)
                    .child(t_with(
                        "upload.queueProgress",
                        &[
                            ("completed", &completed.to_string()),
                            ("total", &total.to_string()),
                        ],
                    )),
            )
            .when(failed > 0, |d| {
                d.child(
                    div()
                        .flex_none()
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .text_size(px(TEXT_SMALL))
                        .text_color(p.muted_foreground)
                        .child(div().size(px(6.)).rounded_full().bg(p.destructive))
                        .child(t_with("upload.queueFailed", &[("n", &failed.to_string())])),
                )
            })
            .child(div().flex_1())
            .child(
                Button::new("upload-queue-action")
                    .label(if cancels {
                        t("upload.cancelAll")
                    } else {
                        t("upload.clearQueue")
                    })
                    .variant(ButtonVariant::Ghost)
                    .size(ButtonSize::Sm)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if cancels {
                            this.cancel_batch(cx)
                        } else {
                            this.clear_queue(cx)
                        }
                    })),
            )
            .when(can_start, |d| {
                d.child(
                    Button::new("upload-batch-transcribe")
                        .label(t_with(
                            "upload.transcribeFiles",
                            &[("count", &startable.to_string())],
                        ))
                        .size(ButtonSize::Sm)
                        .disabled(start_disabled)
                        .on_click(cx.listener(|this, _, _, cx| this.process_queue(cx))),
                )
            });

        let mut list = div().flex().flex_col().px(px(8.)).pb(px(8.));
        for (index, item) in self.queue.iter().enumerate() {
            list = list.child(self.render_queue_row(index, item, &p, cx));
        }

        surface_card(&p)
            .flex()
            .flex_col()
            .child(header)
            .when(show_progress, |d| {
                d.child(
                    div()
                        .mx(px(16.))
                        .mb(px(6.))
                        .h(px(3.))
                        .rounded_full()
                        .overflow_hidden()
                        .bg(p.hover_wash(0.06))
                        .child(
                            div()
                                .h_full()
                                .w(relative(progress))
                                .rounded_full()
                                .bg(p.foreground.opacity(0.6)),
                        ),
                )
            })
            .child(list)
            .when(unsaved, |d| {
                d.child(
                    div()
                        .px(px(16.))
                        .pb(px(12.))
                        .text_size(px(TEXT_SMALL))
                        .line_height(px(LINE_SMALL))
                        .text_color(p.subtle_foreground)
                        .child(t("controlPanel.history.dataRetentionDisabled")),
                )
            })
            .into_any_element()
    }

    fn render_queue_row(
        &self,
        index: usize,
        item: &QueueItem,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let status_icon = match item.status {
            ItemStatus::Done => icon("check", 13., p.success).into_any_element(),
            ItemStatus::Error => icon("alert-circle", 13., p.destructive).into_any_element(),
            ItemStatus::Queued => icon("clock", 13., p.subtle_foreground).into_any_element(),
            ItemStatus::Cancelled => {
                icon("circle-slash", 13., p.subtle_foreground).into_any_element()
            }
            ItemStatus::Transcribing => {
                spinner(("upload-batch-spin", index), 13., p.muted_foreground).into_any_element()
            }
        };
        let id = item.id.clone();
        // Unsaved results can only be copied; saved ones open in History.
        let copy = item
            .text
            .clone()
            .filter(|_| item.transcription_id.is_none());
        let in_history = item
            .transcription_id
            .filter(|_| item.status == ItemStatus::Done);
        // The line under the name: what went wrong, the user's own stop, or
        // how long the running file has taken.
        let note = match item.status {
            ItemStatus::Error => item.error.clone().map(|e| (e, p.muted_foreground)),
            ItemStatus::Cancelled => Some((t("upload.batchCancelled"), p.subtle_foreground)),
            ItemStatus::Transcribing => item.started.map(|started| {
                (
                    t_with(
                        "upload.transcribingElapsed",
                        &[("time", &format_elapsed(started.elapsed()))],
                    ),
                    p.subtle_foreground,
                )
            }),
            ItemStatus::Queued | ItemStatus::Done => None,
        };
        let quiet = matches!(item.status, ItemStatus::Queued | ItemStatus::Cancelled);
        div()
            .flex()
            .items_center()
            .gap(px(10.))
            .min_h(px(40.))
            .px(px(8.))
            .py(px(4.))
            .child(
                div()
                    .flex_none()
                    .w(px(16.))
                    .flex()
                    .justify_center()
                    .child(status_icon),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .truncate()
                            .text_size(px(TEXT_BODY))
                            .text_color(if quiet {
                                p.muted_foreground
                            } else {
                                p.foreground
                            })
                            .child(item.name.clone()),
                    )
                    .when_some(note, |d, (note, color)| {
                        d.child(
                            div()
                                .text_size(px(TEXT_SMALL))
                                .line_height(px(LINE_SMALL))
                                .font_features(tabular_nums())
                                .text_color(color)
                                .child(note),
                        )
                    }),
            )
            .when_some(in_history, |d, id| {
                d.child(
                    Button::new(("upload-batch-history", index))
                        .label(t("upload.viewInHistory"))
                        .variant(ButtonVariant::Ghost)
                        .size(ButtonSize::Sm)
                        .on_click(
                            cx.listener(move |_, _, _, cx| cx.emit(UploadEvent::RevealHistory(id))),
                        ),
                )
            })
            .when_some(copy, |d, text| {
                d.child(
                    Button::new(("upload-batch-copy", index))
                        .label(t("upload.copyText"))
                        .icon("copy", 13.)
                        .variant(ButtonVariant::Ghost)
                        .size(ButtonSize::Sm)
                        .on_click(move |_, _, cx| copy_with_toast(text.clone(), cx)),
                )
            })
            .when(item.status == ItemStatus::Queued, |d| {
                d.child(
                    Button::new(("upload-batch-remove", index))
                        .icon("trash2", 13.)
                        .aria_label(t_with("upload.removeFromQueue", &[("name", &item.name)]))
                        .variant(ButtonVariant::Ghost)
                        .size(ButtonSize::Sm)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.queue.retain(|i| i.id != id);
                            cx.notify();
                        })),
                )
            })
    }
}

impl UploadView {
    /// Where finished transcripts go, under the idle drop target, so the
    /// page answers "what happens next" before a file is chosen. It lines up
    /// with the settings strip's text, and its link is plain text so the line
    /// carries no second glyph.
    fn render_destination(&mut self, cx: &mut Context<Self>) -> Div {
        let p = cx.palette();
        let saving = AppState::settings(cx).data_retention_enabled;
        div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_x(px(6.))
            .min_h(px(44.))
            .pl(px(16.))
            .pr(px(8.))
            .py(px(7.))
            .text_size(px(TEXT_SMALL))
            .text_color(p.subtle_foreground)
            .when(!saving, |d| {
                d.child(icon("alert-circle", 12., p.subtle_foreground))
            })
            .child(t(if saving {
                "upload.savesToHistory"
            } else {
                "upload.historyOff"
            }))
            .when(saving, |d| {
                // Set like the settings action above it, so the card's two
                // actions share one right edge and read as actions at rest.
                d.child(div().flex_1()).child(
                    Button::new("upload-open-history")
                        .label(t("upload.openHistory"))
                        .icon("clock", 13.)
                        .variant(ButtonVariant::Ghost)
                        .size(ButtonSize::Sm)
                        .on_click(cx.listener(|_, _, _, cx| cx.emit(UploadEvent::OpenHistory))),
                )
            })
    }
}

impl Render for UploadView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let stage = self.render_stage_panel(window, cx);
        // The destination line only answers "where does this go" before a
        // result or a queue answers it instead.
        let busy = !self.queue.is_empty() || matches!(self.stage, Stage::Complete { .. });
        let settings = self.render_settings(!busy, cx);
        let queue = (!self.queue.is_empty()).then(|| self.render_queue(window, cx));
        page_container()
            .when(!busy, |d| {
                d.min_h(window.viewport_size().height - px(super::theme::WORKSPACE_TOP_BAR_HEIGHT))
            })
            .pb(px(32.))
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(stage)
            // Once the target reaches its height cap, the information strip
            // stays above the normal bottom inset instead of leaving a blank footer.
            .child(
                div()
                    .flex_none()
                    .when(!busy, |d| d.mt_auto())
                    .child(settings),
            )
            .children(queue)
    }
}

/// Drag-over styles on the stage panel's children key off this group.
const STAGE_GROUP: &str = "upload-stage";

/// The stage panel's floor while a queue is listed below it.
const STAGE_COMPACT_HEIGHT: f32 = 220.;

/// Width of the stage's centered text column.
const GROUP_MAX_WIDTH: f32 = super::theme::UPLOAD_COPY_MAX_WIDTH;

/// Side of the icon tile that heads each stage.
const GLYPH_TILE: f32 = 44.;

/// Height of the transcribing waveform, in px.
const WAVE_HEIGHT: f32 = 40.;

/// Bar heights of the transcribing waveform as fractions of `WAVE_HEIGHT`.
/// Fixed so the motif stays put between renders.
const WAVE: [f32; 28] = [
    0.25, 0.22, 0.24, 0.26, 0.54, 0.52, 0.73, 0.44, 0.55, 0.47, 0.4, 0.36, 0.63, 0.73, 0.83, 0.89,
    0.38, 0.87, 0.79, 0.8, 0.71, 0.58, 0.39, 0.56, 0.45, 0.43, 0.21, 0.18,
];

fn panel_border(p: &Palette) -> Hsla {
    p.border.opacity(if p.dark { 1. } else { 0.8 })
}

/// Let the target grow while keeping the information strip in view.
fn stage_height(window: &Window) -> f32 {
    f32::from(
        window.viewport_size().height
            - px(super::theme::WORKSPACE_TOP_BAR_HEIGHT + super::theme::PAGE_TOP + 32. + 12. + 90.),
    )
    .clamp(
        super::theme::UPLOAD_STAGE_MIN_HEIGHT,
        super::theme::UPLOAD_STAGE_MAX_HEIGHT,
    )
}

/// The stage panel's body: one centered group of glyph, copy and actions.
fn stage_body() -> Div {
    div()
        .flex_1()
        .w_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .px(px(32.))
        .py(px(32.))
        .text_center()
}

fn stage_title(text: impl Into<SharedString>, p: &Palette) -> Div {
    div()
        .max_w(px(GROUP_MAX_WIDTH))
        .text_size(px(TEXT_TITLE))
        .font_weight(FontWeight::SEMIBOLD)
        .line_height(px(LINE_TITLE))
        .text_color(p.foreground)
        .child(text.into())
}

fn stage_text(text: impl Into<SharedString>, p: &Palette) -> Div {
    div()
        .max_w(px(GROUP_MAX_WIDTH))
        .text_size(px(TEXT_BODY))
        .line_height(px(crate::ui::theme::LINE_BODY))
        .text_color(p.muted_foreground)
        .child(text.into())
}

fn stage_hint(text: impl Into<SharedString>, p: &Palette) -> Div {
    div()
        .max_w(px(GROUP_MAX_WIDTH))
        .text_size(px(TEXT_SMALL))
        .line_height(px(LINE_SMALL))
        .text_color(p.subtle_foreground)
        .child(text.into())
}

/// The raised tile that heads a stage.
fn glyph_tile(name: &str, color: Hsla, p: &Palette) -> Div {
    div()
        .size(px(GLYPH_TILE))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(RADIUS_LG))
        .border_1()
        .border_color(panel_border(p))
        .bg(if p.dark {
            p.surface_raised
        } else {
            p.background
        })
        .shadow(vec![drop(0., 1., 2., 0.05)])
        .child(icon(name, 20., color))
}

/// Covers the stage while files hover over it, so the target answers the
/// drag at once. It has no listeners, so it never takes the pointer.
fn drop_overlay(p: &Palette) -> Div {
    div()
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .rounded(px(RADIUS_XL - 1.))
        .bg(p.muted)
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(16.))
        .opacity(0.)
        .group_drag_over::<ExternalPaths>(STAGE_GROUP, |s| s.opacity(1.))
        .child(
            div()
                .size(px(GLYPH_TILE))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(RADIUS_LG))
                .bg(p.foreground)
                .child(icon("upload", 20., p.background)),
        )
        .child(stage_title(t("upload.releaseToAdd"), p))
}

/// A rolling waveform that stands in for progress while a file transcribes.
/// It holds still under Reduce Motion; the title and Cancel button still
/// say work is under way.
fn live_waveform(p: &Palette, cx: &App) -> Div {
    let still = cx.reduce_motion();
    let color = p.foreground.opacity(if p.dark { 0.42 } else { 0.36 });
    let mut bars = div().h(px(WAVE_HEIGHT)).flex().items_center().gap(px(3.));
    for (i, level) in WAVE.iter().copied().enumerate() {
        let height = (WAVE_HEIGHT * level).max(3.);
        let offset = i as f32 / WAVE.len() as f32;
        let bar = div().w(px(3.)).h(px(height)).rounded(px(1.5)).bg(color);
        bars = if still {
            bars.child(bar)
        } else {
            bars.child(bar.with_animation(
                ("upload-wave", i),
                motion(1600, cx).repeat(),
                move |d, t| {
                    let k = 0.5 - 0.5 * ((t - offset) * TAU).cos();
                    d.h(px(height * (0.45 + 0.55 * k)))
                },
            ))
        };
    }
    bars
}

fn stage_key(stage: &Stage) -> &'static str {
    match stage {
        Stage::Idle => "idle",
        Stage::Selected => "selected",
        Stage::Transcribing => "transcribing",
        Stage::Complete { .. } => "complete",
        Stage::Error(_) => "error",
    }
}
