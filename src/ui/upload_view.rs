//! Upload: transcribe audio and video files one at a time or as a batch.
//!
//! The view entity outlives navigation, so a running batch keeps draining
//! while the user looks at other pages.

use std::path::PathBuf;
use std::time::Duration;

use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use whisper::db::SaveTranscriptionOptions;
use whisper::i18n::{t, t_with};
use whisper::services::upload::{
    FileTranscriptionRequest, UploadError, cancel_upload_transcription, is_supported_upload_file,
    transcribe_audio_file,
};

use crate::app_state::AppState;

use super::components::{Button, ButtonSize, ButtonVariant, f32_tx, icon, spinner};
use super::history_view::copy_with_toast;
use super::theme::{
    FONT_VOICE, LINE_READING, LINE_SMALL, PaletteExt, TEXT_BODY, TEXT_CAPTION, TEXT_DISPLAY,
    TEXT_READING, TEXT_SMALL, ease_out,
};

pub enum UploadEvent {
    OpenHistory,
    OpenSettings(&'static str),
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
}

pub struct UploadView {
    stage: Stage,
    file: Option<SelectedFile>,
    run_id: u64,
    active_request: Option<String>,
    skipped_notice: Option<String>,
    queue: Vec<QueueItem>,
    batch_processing: bool,
    batch_run: u64,
    batch_request: Option<String>,
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
    if err.code.as_deref() == Some("CUSTOM_ENDPOINT_INVALID") {
        return t("upload.customEndpointInvalid");
    }
    if let Some(key) = &err.message_key
        && let Some(text) = whisper::i18n::try_t(key)
    {
        return text;
    }
    if err.message.is_empty() {
        t("upload.transcriptionFailed")
    } else {
        err.message.clone()
    }
}

fn build_request(path: PathBuf, request_id: String, cx: &App) -> FileTranscriptionRequest {
    let s = AppState::settings(cx);
    FileTranscriptionRequest {
        file_path: path,
        language: whisper::text::language::get_base_language_code(&s.preferred_language)
            .unwrap_or_default()
            .to_string(),
        remote_transcription_url: s.remote_transcription_url.clone(),
        remote_transcription_model: s.remote_transcription_model.clone(),
        request_id: Some(request_id),
    }
}

/// Saves an upload's raw transcript to History when retention is on.
/// Uploads skip cleanup, Snippets and Insights.
fn save_upload(text: &str, cx: &mut App) -> Result<Option<i64>, String> {
    let state = AppState::global(cx);
    if !state.read(cx).settings.data_retention_enabled {
        return Ok(None);
    }
    let db = state.read(cx).db.clone();
    let options = SaveTranscriptionOptions {
        route_kind: Some("upload".into()),
        ..Default::default()
    };
    match db.save_transcription(text, None, options) {
        Ok(item) => {
            let id = item.id;
            state.update(cx, |s, cx| s.upsert_history_item(item, cx));
            Ok(Some(id))
        }
        Err(err) => Err(err.to_string()),
    }
}

fn run_transcription(
    request: FileTranscriptionRequest,
) -> impl std::future::Future<Output = Result<String, UploadError>> {
    let handle = whisper::services::runtime::spawn(transcribe_audio_file(request));
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
        }
    }

    fn provider_ready(cx: &App) -> bool {
        !AppState::settings(cx)
            .remote_transcription_url
            .trim()
            .is_empty()
    }

    fn model_label(cx: &App) -> String {
        let name = t("settings.transcription.modes.selfHosted");
        let model = AppState::settings(cx).remote_transcription_model;
        if model.trim().is_empty() {
            name
        } else {
            format!("{name} · {model}")
        }
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
                });
            }
        }
        cx.notify();
    }

    fn browse(&mut self, cx: &mut Context<Self>) {
        // An async panel keeps GPUI's run loop free while the dialog is open.
        cx.spawn(async move |this, cx| {
            if let Some(paths) = whisper::services::upload::select_audio_files(true).await {
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
        let request_id = uuid::Uuid::new_v4().to_string();
        self.active_request = Some(request_id.clone());
        self.stage = Stage::Transcribing;
        cx.notify();
        let request = build_request(file.path.clone(), request_id.clone(), cx);
        let task = run_transcription(request);
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                if this.active_request.as_deref() == Some(request_id.as_str()) {
                    this.active_request = None;
                }
                if run != this.run_id {
                    return;
                }
                this.stage = match result {
                    Ok(text) if !text.trim().is_empty() => match save_upload(&text, cx) {
                        Ok(id) => Stage::Complete {
                            text,
                            transcription_id: id,
                            save_error: None,
                        },
                        Err(_) => Stage::Complete {
                            text,
                            transcription_id: None,
                            save_error: Some(t("upload.errorOccurred")),
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
        if let Some(id) = self.active_request.take() {
            cancel_upload_transcription(&id);
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
                        let request_id = uuid::Uuid::new_v4().to_string();
                        let request = build_request(item.path.clone(), request_id.clone(), cx);
                        let id = item.id.clone();
                        this.batch_request = Some(request_id);
                        cx.notify();
                        Some((id, request))
                    })
                    .ok()
                    .flatten();
                let Some((id, request)) = next else { break };
                processed.insert(id.clone());
                let result = run_transcription(request).await;
                let still_current = this
                    .update(cx, |this, cx| {
                        this.batch_request = None;
                        if run != this.batch_run {
                            return false;
                        }
                        let outcome = match &result {
                            Ok(text) if !text.trim().is_empty() => Some(save_upload(text, cx)),
                            _ => None,
                        };
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
                                    item.error = Some(t("upload.errorOccurred"));
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
        if let Some(id) = self.batch_request.take() {
            cancel_upload_transcription(&id);
        }
        self.batch_processing = false;
        for item in &mut self.queue {
            if !matches!(item.status, ItemStatus::Done | ItemStatus::Error) {
                item.status = ItemStatus::Error;
                item.error = Some(t("upload.batchCancelled"));
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

    fn render_idle(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let p = cx.palette();
        let label = Self::model_label(cx);
        div()
            .id("upload-card")
            .rounded(px(12.))
            .overflow_hidden()
            .bg(p.card)
            .border_1()
            .border_color(if p.dark {
                p.white.opacity(0.12)
            } else {
                p.foreground.opacity(0.1)
            })
            .drag_over::<ExternalPaths>(move |s, _, _, _| {
                s.border_color(p.primary.opacity(0.3))
                    .bg(p.primary.opacity(if p.dark { 0.06 } else { 0.04 }))
            })
            .on_drop(cx.listener(|this, paths: &ExternalPaths, _, cx| {
                this.accept_paths(paths.paths().to_vec(), true, cx);
            }))
            .child(
                div()
                    .id("upload-drop")
                    .group("upload-drop")
                    .relative()
                    .px(px(36.))
                    .pt(px(44.))
                    .pb(px(36.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(10.))
                    .cursor_pointer()
                    .hover(|s| s.bg(p.hover_wash(0.02)))
                    .on_click(cx.listener(|this, _, _, cx| this.browse(cx)))
                    .child(
                        div()
                            .mb(px(8.))
                            .size(px(52.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(13.))
                            .border_1()
                            .border_color(if p.dark {
                                p.white.opacity(0.12)
                            } else {
                                p.foreground.opacity(0.1)
                            })
                            .bg(if p.dark {
                                p.white.opacity(0.06)
                            } else {
                                p.foreground.opacity(0.04)
                            })
                            .child(icon("upload", 22., p.foreground.opacity(0.8))),
                    )
                    .child(
                        div()
                            .font_family(FONT_VOICE)
                            .text_size(px(TEXT_DISPLAY))
                            .font_weight(FontWeight::MEDIUM)
                            .line_height(px(30.))
                            .text_color(p.foreground)
                            .child(t("upload.title")),
                    )
                    .child(
                        div()
                            .text_size(px(TEXT_BODY))
                            .text_color(p.foreground.opacity(0.8))
                            .child(t("upload.dropOrBrowse")),
                    )
                    .child(
                        div()
                            .max_w(px(320.))
                            .text_center()
                            .text_size(px(TEXT_SMALL))
                            .line_height(px(LINE_SMALL))
                            .text_color(p.foreground.opacity(0.6))
                            .child(t("upload.supportedFormats")),
                    ),
            )
            .child(
                div()
                    .id("upload-model")
                    .group("upload-model")
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(12.))
                    .py(px(12.))
                    .pl(px(20.))
                    .pr(px(16.))
                    .border_t_1()
                    .border_color(if p.dark {
                        p.white.opacity(0.08)
                    } else {
                        p.foreground.opacity(0.08)
                    })
                    .bg(p.hover_wash(0.02))
                    .cursor_pointer()
                    .hover(|s| s.bg(p.hover_wash(0.04)))
                    .on_click(cx.listener(|_, _, _, cx| {
                        cx.emit(UploadEvent::OpenSettings("uploadTranscription"))
                    }))
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_size(px(TEXT_BODY))
                            .text_color(p.foreground.opacity(0.85))
                            .child(label),
                    )
                    .child(
                        div()
                            .flex_none()
                            .rounded(px(6.))
                            .border_1()
                            .border_color(if p.dark {
                                p.white.opacity(0.12)
                            } else {
                                p.foreground.opacity(0.12)
                            })
                            .px(px(10.))
                            .py(px(4.))
                            .text_size(px(TEXT_SMALL))
                            .text_color(p.foreground.opacity(0.85))
                            .group_hover("upload-model", |s| s.text_color(p.foreground))
                            .child(t("upload.changeModel")),
                    ),
            )
            .into_any_element()
    }

    fn render_no_provider(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let p = cx.palette();
        div()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(16.))
            .py(px(8.))
            .child(
                div()
                    .size(px(40.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(10.))
                    .border_1()
                    .border_color(if p.dark {
                        p.white.opacity(0.1)
                    } else {
                        p.foreground.opacity(0.08)
                    })
                    .bg(if p.dark {
                        p.white.opacity(0.05)
                    } else {
                        p.foreground.opacity(0.035)
                    })
                    .child(icon("settings", 17., p.foreground.opacity(0.45))),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .child(
                        div()
                            .mb(px(4.))
                            .text_size(px(TEXT_SMALL))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(t("upload.noProviderTitle")),
                    )
                    .child(
                        div()
                            .max_w(px(240.))
                            .text_center()
                            .text_size(px(TEXT_SMALL))
                            .line_height(px(LINE_SMALL))
                            .text_color(p.foreground.opacity(0.45))
                            .child(t("upload.customEndpointInvalid")),
                    ),
            )
            .child(
                Button::new("no-provider")
                    .label(t("upload.noProviderAction"))
                    .size(ButtonSize::Sm)
                    .h(28.)
                    .on_click(cx.listener(|_, _, _, cx| {
                        cx.emit(UploadEvent::OpenSettings("uploadTranscription"))
                    })),
            )
            .into_any_element()
    }

    fn render_selected(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let p = cx.palette();
        let file = self.file.clone().expect("selected file");
        let label = Self::model_label(cx);
        let disabled = self.batch_processing || !Self::provider_ready(cx);
        div()
            .child(
                div()
                    .mb(px(12.))
                    .p(px(16.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(if p.dark {
                        p.white.opacity(0.1)
                    } else {
                        p.foreground.opacity(0.08)
                    })
                    .bg(if p.dark {
                        p.white.opacity(0.03)
                    } else {
                        p.surface_1.opacity(0.4)
                    })
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(12.))
                            .child(
                                div()
                                    .size(px(36.))
                                    .flex_none()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded(px(8.))
                                    .border_1()
                                    .border_color(p.primary.opacity(if p.dark {
                                        0.15
                                    } else {
                                        0.1
                                    }))
                                    .bg(p.primary.opacity(if p.dark { 0.12 } else { 0.08 }))
                                    .child(icon("file-audio", 15., p.primary.opacity(0.6))),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .child(
                                        div()
                                            .truncate()
                                            .text_size(px(TEXT_SMALL))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(p.foreground.opacity(0.7))
                                            .child(file.name.clone()),
                                    )
                                    .when(!file.size.is_empty(), |d| {
                                        d.child(
                                            div()
                                                .mt(px(2.))
                                                .text_size(px(TEXT_SMALL))
                                                .text_color(p.foreground.opacity(0.45))
                                                .child(file.size.clone()),
                                        )
                                    })
                                    .child(
                                        div()
                                            .id("selected-model")
                                            .mt(px(2.))
                                            .truncate()
                                            .text_size(px(TEXT_SMALL))
                                            .text_color(p.foreground.opacity(0.7))
                                            .underline()
                                            .cursor_pointer()
                                            .hover(|s| s.text_color(p.foreground))
                                            .child(label)
                                            .on_click(cx.listener(|_, _, _, cx| {
                                                cx.emit(UploadEvent::OpenSettings(
                                                    "uploadTranscription",
                                                ))
                                            })),
                                    ),
                            )
                            .child(
                                div()
                                    .id("selected-clear")
                                    .p(px(4.))
                                    .cursor_pointer()
                                    .child(icon("x", 12., p.foreground.opacity(0.45)))
                                    .on_click(cx.listener(|this, _, _, cx| this.reset(cx))),
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .gap(px(8.))
                    .child(
                        Button::new("upload-transcribe")
                            .label(t("upload.transcribe"))
                            .size(ButtonSize::Sm)
                            .disabled(disabled)
                            .on_click(cx.listener(|this, _, _, cx| this.transcribe(cx))),
                    )
                    .child(
                        Button::new("upload-cancel")
                            .label(t("upload.cancel"))
                            .variant(ButtonVariant::Ghost)
                            .size(ButtonSize::Sm)
                            .on_click(cx.listener(|this, _, _, cx| this.reset(cx))),
                    ),
            )
            .into_any_element()
    }

    fn render_transcribing(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let p = cx.palette();
        let name = self.file.as_ref().map(|f| f.name.clone());
        div()
            .flex()
            .flex_col()
            .items_center()
            .child(
                div()
                    .mb(px(20.))
                    .child(spinner("upload-spin", 32., p.primary)),
            )
            .child(
                div()
                    .text_size(px(TEXT_SMALL))
                    .font_weight(FontWeight::MEDIUM)
                    .child(t("upload.transcribing")),
            )
            .when_some(name, |d, name| {
                d.child(
                    div()
                        .mt(px(4.))
                        .max_w(px(200.))
                        .truncate()
                        .text_size(px(TEXT_SMALL))
                        .text_color(p.muted_foreground)
                        .child(name),
                )
            })
            .child(
                div()
                    .mt(px(16.))
                    .text_center()
                    .text_size(px(TEXT_SMALL))
                    .line_height(px(LINE_SMALL))
                    .text_color(p.muted_foreground)
                    .child(t("upload.completionHint")),
            )
            .child(
                div().mt(px(16.)).child(
                    Button::new("cancel-upload")
                        .label(t("upload.cancelTranscription"))
                        .variant(ButtonVariant::Ghost)
                        .size(ButtonSize::Sm)
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
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = cx.palette();
        // The ring fills, then the check draws in, as in `ring-fill`/`draw-check`.
        let ring = f32_tx("upload-ring", 1., 800, ease_out, window, cx);
        let file_name = self.file.as_ref().map(|f| f.name.clone());
        let copy_text = text.clone();
        div()
            .flex()
            .flex_col()
            .items_center()
            .child(
                div()
                    .relative()
                    .size(px(48.))
                    .mb(px(16.))
                    .child(
                        div()
                            .absolute()
                            .inset_0()
                            .rounded_full()
                            .border(px(1.5))
                            .border_color(p.success.opacity(0.15)),
                    )
                    .child(
                        div()
                            .absolute()
                            .inset_0()
                            .rounded_full()
                            .border(px(1.5))
                            .border_color(p.success.opacity(0.6 * ring))
                            .with_animation(
                                "ring-fill",
                                Animation::new(Duration::from_millis(800)).with_easing(ease_out),
                                |d, t| d.opacity(t),
                            ),
                    )
                    .child(
                        div()
                            .absolute()
                            .inset_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon("check", 20., p.success.opacity(0.7)))
                            .with_animation(
                                "draw-check",
                                Animation::new(Duration::from_millis(900)).with_easing(ease_out),
                                |d, t| d.opacity(((t - 0.55) / 0.45).clamp(0., 1.)),
                            ),
                    ),
            )
            .child(
                div()
                    .mb(px(4.))
                    .text_size(px(TEXT_SMALL))
                    .font_weight(FontWeight::MEDIUM)
                    .child(t("upload.transcriptionComplete")),
            )
            .when(transcription_id.is_some(), |d| {
                d.child(
                    div()
                        .mb(px(16.))
                        .text_size(px(TEXT_SMALL))
                        .text_color(p.muted_foreground)
                        .child(t("upload.savedToHistory")),
                )
            })
            .child(
                div()
                    .w_full()
                    .mb(px(16.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(p.border)
                    .bg(p.surface_1)
                    .overflow_hidden()
                    .when_some(file_name, |d, name| {
                        d.child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(8.))
                                .px(px(16.))
                                .py(px(10.))
                                .border_b_1()
                                .border_color(p.border)
                                .child(icon("file-audio", 14., p.muted_foreground))
                                .child(
                                    div()
                                        .truncate()
                                        .text_size(px(TEXT_SMALL))
                                        .text_color(p.muted_foreground)
                                        .child(name),
                                ),
                        )
                    })
                    .child(
                        div()
                            .id("upload-result")
                            .max_h(px(320.))
                            .overflow_y_scroll()
                            .p(px(16.))
                            .font_family(FONT_VOICE)
                            .text_size(px(TEXT_READING))
                            .line_height(px(LINE_READING))
                            .text_color(p.foreground)
                            .child(text),
                    ),
            )
            .when_some(save_error.clone(), |d, err| {
                d.child(
                    div()
                        .mb(px(16.))
                        .text_center()
                        .text_size(px(TEXT_SMALL))
                        .text_color(p.destructive)
                        .child(err),
                )
            })
            .when(transcription_id.is_none() && save_error.is_none(), |d| {
                d.child(
                    div()
                        .mb(px(16.))
                        .text_center()
                        .text_size(px(TEXT_SMALL))
                        .text_color(p.muted_foreground)
                        .child(t("controlPanel.history.dataRetentionDisabled")),
                )
            })
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .justify_center()
                    .gap(px(8.))
                    .child(
                        Button::new("copy-full")
                            .label(t("upload.copyFullText"))
                            .size(ButtonSize::Sm)
                            .on_click(move |_, _, cx| copy_with_toast(copy_text.clone(), cx)),
                    )
                    .when(transcription_id.is_some(), |d| {
                        d.child(
                            Button::new("view-history")
                                .label(t("upload.viewInHistory"))
                                .variant(ButtonVariant::Ghost)
                                .size(ButtonSize::Sm)
                                .on_click(
                                    cx.listener(|_, _, _, cx| cx.emit(UploadEvent::OpenHistory)),
                                ),
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
        div()
            .child(
                div()
                    .mb(px(16.))
                    .p(px(16.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(p.destructive.opacity(if p.dark { 0.2 } else { 0.15 }))
                    .bg(p.destructive.opacity(if p.dark { 0.05 } else { 0.03 }))
                    .child(
                        div()
                            .flex()
                            .items_start()
                            .gap(px(10.))
                            .child(div().mt(px(2.)).child(icon(
                                "alert-circle",
                                14.,
                                p.destructive.opacity(0.5),
                            )))
                            .child(
                                div()
                                    .flex_1()
                                    .text_size(px(TEXT_SMALL))
                                    .line_height(px(LINE_SMALL))
                                    .text_color(p.destructive.opacity(0.7))
                                    .child(error),
                            )
                            .child(
                                div()
                                    .id("error-dismiss")
                                    .p(px(2.))
                                    .cursor_pointer()
                                    .child(icon("x", 11., p.foreground.opacity(0.45)))
                                    .on_click(cx.listener(|this, _, _, cx| this.reset(cx))),
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .gap(px(8.))
                    .child(
                        Button::new("upload-retry")
                            .label(t("upload.retry"))
                            .variant(ButtonVariant::Ghost)
                            .size(ButtonSize::Sm)
                            .h(28.)
                            .on_click(cx.listener(|this, _, _, cx| {
                                if this.file.is_some() {
                                    this.transcribe(cx)
                                } else {
                                    this.reset(cx)
                                }
                            })),
                    )
                    .child(
                        Button::new("upload-start-over")
                            .label(t("upload.startOver"))
                            .variant(ButtonVariant::Ghost)
                            .size(ButtonSize::Sm)
                            .h(28.)
                            .on_click(cx.listener(|this, _, _, cx| this.reset(cx))),
                    ),
            )
            .into_any_element()
    }

    fn render_queue(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let p = cx.palette();
        let total = self.queue.len();
        let completed = self
            .queue
            .iter()
            .filter(|i| i.status == ItemStatus::Done)
            .count();
        let failed = self
            .queue
            .iter()
            .filter(|i| i.status == ItemStatus::Error)
            .count();
        let all_done = total > 0 && completed + failed == total;
        let progress = if total > 0 {
            (completed + failed) as f32 / total as f32
        } else {
            0.
        };
        let progress = f32_tx("batch-progress", progress, 500, ease_out, window, cx);
        let has_queued = self.queue.iter().any(|i| i.status == ItemStatus::Queued);
        let unsaved = self
            .queue
            .iter()
            .any(|i| i.status == ItemStatus::Done && i.transcription_id.is_none());

        let mut list = div()
            .id("batch-list")
            .max_h(px(300.))
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap(px(4.));
        for (index, item) in self.queue.iter().enumerate() {
            let status_icon = match item.status {
                ItemStatus::Done => icon("check", 12., p.success.opacity(0.7)).into_any_element(),
                ItemStatus::Error => icon("x", 12., p.destructive.opacity(0.7)).into_any_element(),
                ItemStatus::Queued => {
                    icon("clock", 12., p.foreground.opacity(0.45)).into_any_element()
                }
                ItemStatus::Transcribing => {
                    spinner(("batch-spin", index), 12., p.primary.opacity(0.6)).into_any_element()
                }
            };
            let id = item.id.clone();
            let copy = item
                .text
                .clone()
                .filter(|_| item.transcription_id.is_none());
            list =
                list.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .px(px(10.))
                        .py(px(6.))
                        .rounded(px(6.))
                        .text_size(px(TEXT_SMALL))
                        .border_1()
                        .border_color(if item.status == ItemStatus::Error {
                            p.destructive.opacity(0.15)
                        } else if p.dark {
                            p.white.opacity(0.1)
                        } else {
                            p.foreground.opacity(0.04)
                        })
                        .bg(if p.dark {
                            p.white.opacity(0.02)
                        } else {
                            p.surface_1.opacity(0.3)
                        })
                        .child(status_icon)
                        .child(
                            div()
                                .flex_1()
                                .truncate()
                                .text_color(p.foreground.opacity(0.6))
                                .child(item.name.clone()),
                        )
                        .when(
                            item.status == ItemStatus::Done && item.transcription_id.is_some(),
                            |d| {
                                d.child(
                                    div()
                                        .id(("batch-history", index))
                                        .text_size(px(TEXT_CAPTION))
                                        .text_color(p.primary.opacity(0.5))
                                        .cursor_pointer()
                                        .hover(|s| s.text_color(p.primary.opacity(0.7)))
                                        .child(t("controlPanel.history.sectionTitle"))
                                        .on_click(cx.listener(|_, _, _, cx| {
                                            cx.emit(UploadEvent::OpenHistory)
                                        })),
                                )
                            },
                        )
                        .when_some(copy, |d, text| {
                            d.child(
                                div()
                                    .id(("batch-copy", index))
                                    .text_size(px(TEXT_CAPTION))
                                    .text_color(p.primary.opacity(0.5))
                                    .cursor_pointer()
                                    .hover(|s| s.text_color(p.primary.opacity(0.7)))
                                    .child(t("controlPanel.history.copyText"))
                                    .on_click(move |_, _, cx| copy_with_toast(text.clone(), cx)),
                            )
                        })
                        .when_some(
                            item.error
                                .clone()
                                .filter(|_| item.status == ItemStatus::Error),
                            |d, err| {
                                d.child(
                                    div()
                                        .max_w(px(80.))
                                        .truncate()
                                        .text_size(px(TEXT_CAPTION))
                                        .text_color(p.destructive.opacity(0.5))
                                        .child(err),
                                )
                            },
                        )
                        .when(item.status == ItemStatus::Queued, |d| {
                            d.child(
                                div()
                                    .id(("batch-remove", index))
                                    .cursor_pointer()
                                    .child(icon("trash2", 10., p.foreground.opacity(0.45)))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.queue.retain(|i| i.id != id);
                                        cx.notify();
                                    })),
                            )
                        }),
                );
        }

        div()
            .mt(px(12.))
            .child(
                div()
                    .mb(px(12.))
                    .child(
                        div()
                            .mb(px(6.))
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .flex()
                                    .text_size(px(TEXT_SMALL))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(p.foreground.opacity(0.5))
                                    .child(t_with(
                                        "upload.queueProgress",
                                        &[
                                            ("completed", &completed.to_string()),
                                            ("total", &total.to_string()),
                                        ],
                                    ))
                                    .when(failed > 0, |d| {
                                        d.child(
                                            div()
                                                .font_weight(FontWeight::NORMAL)
                                                .text_color(p.destructive.opacity(0.5))
                                                .child(format!(
                                                    " · {}",
                                                    t_with(
                                                        "upload.queueFailed",
                                                        &[("n", &failed.to_string())]
                                                    )
                                                )),
                                        )
                                    }),
                            )
                            .when(all_done, |d| {
                                d.child(
                                    Button::new("clear-queue")
                                        .label(t("upload.clearQueue"))
                                        .variant(ButtonVariant::Ghost)
                                        .size(ButtonSize::Sm)
                                        .h(24.)
                                        .text_size(10.)
                                        .on_click(
                                            cx.listener(|this, _, _, cx| this.clear_queue(cx)),
                                        ),
                                )
                            }),
                    )
                    .child(
                        div()
                            .w_full()
                            .h(px(3.))
                            .rounded_full()
                            .overflow_hidden()
                            .bg(if p.dark {
                                p.white.opacity(0.05)
                            } else {
                                p.foreground.opacity(0.05)
                            })
                            .child(
                                div()
                                    .h_full()
                                    .w(relative(progress))
                                    .rounded_full()
                                    .bg(p.primary.opacity(0.5)),
                            ),
                    ),
            )
            .child(list)
            .when(unsaved, |d| {
                d.child(
                    div()
                        .mt(px(8.))
                        .text_center()
                        .text_size(px(TEXT_CAPTION))
                        .text_color(p.foreground.opacity(0.45))
                        .child(t("controlPanel.history.dataRetentionDisabled")),
                )
            })
            .when(self.batch_processing && !all_done, |d| {
                d.child(
                    div().mt(px(12.)).flex().justify_center().child(
                        Button::new("cancel-batch")
                            .label(t("upload.cancelAll"))
                            .variant(ButtonVariant::Ghost)
                            .size(ButtonSize::Sm)
                            .h(28.)
                            .on_click(cx.listener(|this, _, _, cx| this.cancel_batch(cx))),
                    ),
                )
            })
            .when(!self.batch_processing && has_queued, |d| {
                let disabled = !Self::provider_ready(cx) || self.stage == Stage::Transcribing;
                d.child(
                    div().mt(px(12.)).flex().justify_center().child(
                        Button::new("batch-transcribe")
                            .label(t("upload.transcribe"))
                            .size(ButtonSize::Sm)
                            .disabled(disabled)
                            .on_click(cx.listener(|this, _, _, cx| this.process_queue(cx))),
                    ),
                )
            })
            .into_any_element()
    }
}

impl Render for UploadView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = cx.palette();
        let ready = Self::provider_ready(cx);
        let stage = self.stage.clone();
        let body = match stage.clone() {
            Stage::Idle if !ready => self.render_no_provider(cx),
            Stage::Idle => self.render_idle(cx),
            Stage::Selected => self.render_selected(cx),
            Stage::Transcribing => self.render_transcribing(cx),
            Stage::Complete {
                text,
                transcription_id,
                save_error,
            } => self.render_complete(text, transcription_id, save_error, window, cx),
            Stage::Error(error) => self.render_error(error, cx),
        };
        let wide = matches!(stage, Stage::Complete { .. });
        let narrow = !matches!(stage, Stage::Complete { .. } | Stage::Idle);
        let queue = (!self.queue.is_empty()).then(|| self.render_queue(window, cx));
        let notice = self.skipped_notice.clone();
        let viewport = window.viewport_size().height - px(66.);

        div()
            .w_full()
            .min_h(viewport)
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .px(px(24.))
            .child(
                div()
                    .w_full()
                    .max_w(px(if wide { 672. } else { 448. }))
                    .when(wide, |d| d.py(px(24.)))
                    .child(
                        div()
                            .mx_auto()
                            .when(narrow, |d| d.max_w(px(320.)))
                            .child(body)
                            .when_some(notice, |d, notice| {
                                d.child(
                                    div()
                                        .mt(px(8.))
                                        .text_center()
                                        .text_size(px(TEXT_CAPTION))
                                        .text_color(p.warning.opacity(0.6))
                                        .child(notice),
                                )
                            })
                            .children(queue),
                    )
                    .with_animation(
                        SharedString::from(format!("upload-stage-{}", stage_key(&stage))),
                        Animation::new(Duration::from_millis(350)).with_easing(ease_out),
                        |d, t| d.opacity(t).mt(px(8. * (1. - t))),
                    ),
            )
    }
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
