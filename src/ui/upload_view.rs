//! Upload: transcribe audio and video files one at a time or as a batch.
//!
//! The view entity outlives navigation, so a running batch keeps draining
//! while the user looks at other pages.

use std::f32::consts::TAU;
use std::path::PathBuf;
use std::time::Duration;

use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::i18n::{t, t_with};
use crate::services::upload::{
    FileTranscriptionRequest, UploadError, cancel_upload_transcription, is_supported_upload_file,
    transcribe_audio_file,
};

use crate::app_state::AppState;

use super::components::{Button, ButtonSize, ButtonVariant, drop, f32_tx, icon, spinner};
use super::history_view::copy_with_toast;
use super::theme::{
    DUR_SHEET, FONT_MONO, FONT_VOICE, LINE_READING, LINE_SMALL, Palette, PaletteExt, RADIUS_MD,
    RADIUS_XL, TEXT_BODY, TEXT_CAPTION, TEXT_DISPLAY, TEXT_READING, TEXT_SMALL, TEXT_TITLE,
    ease_out,
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

fn build_request(path: PathBuf, request_id: String, cx: &App) -> FileTranscriptionRequest {
    let s = AppState::settings(cx);
    FileTranscriptionRequest {
        file_path: path,
        language: crate::text::language::get_base_language_code(&s.preferred_language)
            .unwrap_or_default()
            .to_string(),
        remote_transcription_url: s.remote_transcription_url.clone(),
        remote_transcription_model: s.remote_transcription_model.clone(),
        request_id: Some(request_id),
    }
}

/// Saves an upload's raw transcript to History when retention is on.
fn save_upload(text: &str, cx: &mut App) -> Result<Option<i64>, String> {
    AppState::global(cx)
        .update(cx, |s, cx| {
            s.record_history(cx, |history, settings| {
                history.record_upload(text, settings)
            })
        })
        .map_err(|err| err.to_string())
}

fn run_transcription(
    request: FileTranscriptionRequest,
) -> impl std::future::Future<Output = Result<String, UploadError>> {
    let handle = crate::services::runtime::spawn(transcribe_audio_file(request));
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

    fn render_stage_panel(
        &mut self,
        wide: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = cx.palette();
        let ready = Self::provider_ready(cx);
        let stage = self.stage.clone();
        let accepts_drop = ready && stage != Stage::Transcribing;
        let browsable = ready && stage == Stage::Idle;
        let content = match stage.clone() {
            Stage::Idle if !ready => self.render_no_provider(cx),
            Stage::Idle => self.render_idle(cx),
            Stage::Selected => self.render_selected(cx),
            Stage::Transcribing => self.render_transcribing(cx),
            Stage::Complete {
                text,
                transcription_id,
                save_error,
            } => self.render_complete(text, transcription_id, save_error, wide, cx),
            Stage::Error(error) => self.render_error(error, cx),
        };
        let notice = self.skipped_notice.clone();
        let _ = window;

        div()
            .id("upload-stage")
            .group(STAGE_GROUP)
            .flex()
            .flex_col()
            .overflow_hidden()
            .rounded(px(RADIUS_XL))
            .border_1()
            .border_color(panel_border(&p))
            .bg(p.card)
            .shadow(vec![drop(0., 1., 2., 0.04)])
            .when(wide, |d| d.flex_grow(1.7).flex_basis(px(0.)).min_w_0())
            .when(!wide, |d| d.w_full().min_h(px(340.)))
            .when(accepts_drop, |d| {
                d.drag_over::<ExternalPaths>(move |s, _, _, _| {
                    s.border_color(p.foreground.opacity(0.35))
                        .bg(p.hover_wash(0.03))
                })
                .on_drop(cx.listener(|this, paths: &ExternalPaths, _, cx| {
                    this.accept_paths(paths.paths().to_vec(), true, cx);
                }))
            })
            .when(browsable, |d| {
                d.cursor_pointer()
                    .hover(|s| s.border_color(p.border_hover))
                    .on_click(cx.listener(|this, _, _, cx| this.browse(cx)))
            })
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .child(content)
                    .with_animation(
                        SharedString::from(format!("upload-stage-{}", stage_key(&stage))),
                        Animation::new(Duration::from_millis(DUR_SHEET)).with_easing(ease_out),
                        |d, t| d.opacity(t),
                    ),
            )
            .when_some(notice, |d, notice| {
                d.child(
                    div()
                        .px(px(28.))
                        .pb(px(16.))
                        .text_size(px(TEXT_CAPTION))
                        .text_color(p.warning.opacity(0.8))
                        .child(notice),
                )
            })
            .into_any_element()
    }

    fn render_idle(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let p = cx.palette();
        stage_body()
            .child(waveform(&p, 0.09, false))
            .child(
                div()
                    .mt(px(24.))
                    .font_family(FONT_VOICE)
                    .text_size(px(TEXT_DISPLAY))
                    .font_weight(FontWeight::MEDIUM)
                    .line_height(px(30.))
                    .text_color(p.foreground)
                    .child(t("upload.dropTitle")),
            )
            .child(
                div()
                    .mt(px(6.))
                    .text_size(px(TEXT_BODY))
                    .text_color(p.muted_foreground)
                    .child(t("upload.dropHint")),
            )
            .child(
                div()
                    .mt(px(2.))
                    .text_size(px(TEXT_SMALL))
                    .line_height(px(LINE_SMALL))
                    .text_color(p.muted_foreground.opacity(0.7))
                    .child(t("upload.supportedFormats")),
            )
            .child(
                // No handler of its own: the click reaches the panel, which browses.
                div().mt(px(18.)).flex().child(
                    Button::new("upload-browse")
                        .label(t("upload.chooseFiles"))
                        .size(ButtonSize::Sm),
                ),
            )
            .into_any_element()
    }

    fn render_no_provider(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let p = cx.palette();
        stage_body()
            .child(waveform(&p, 0.05, false))
            .child(
                div()
                    .mt(px(24.))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(icon("settings", 18., p.muted_foreground))
                    .child(
                        div()
                            .text_size(px(TEXT_TITLE))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(p.foreground)
                            .child(t("upload.noProviderTitle")),
                    ),
            )
            .child(
                div()
                    .mt(px(6.))
                    .max_w(px(360.))
                    .text_size(px(TEXT_SMALL))
                    .line_height(px(LINE_SMALL))
                    .text_color(p.muted_foreground)
                    .child(t("upload.customEndpointInvalid")),
            )
            .child(
                div().mt(px(18.)).flex().child(
                    Button::new("no-provider")
                        .label(t("upload.noProviderAction"))
                        .size(ButtonSize::Sm)
                        .on_click(cx.listener(|_, _, _, cx| {
                            cx.emit(UploadEvent::OpenSettings("uploadTranscription"))
                        })),
                ),
            )
            .into_any_element()
    }

    fn render_selected(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let p = cx.palette();
        let file = self.file.clone().expect("selected file");
        let disabled = self.batch_processing || !Self::provider_ready(cx);
        stage_body()
            .child(waveform(&p, 0.2, false))
            .child(
                div()
                    .mt(px(24.))
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .child(file_badge(&p))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(TEXT_BODY))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(p.foreground)
                                    .child(file.name.clone()),
                            )
                            .when(!file.size.is_empty(), |d| {
                                d.child(
                                    div()
                                        .mt(px(2.))
                                        .text_size(px(TEXT_SMALL))
                                        .text_color(p.muted_foreground)
                                        .child(file.size.clone()),
                                )
                            }),
                    )
                    .child(
                        div()
                            .id("selected-clear")
                            .flex_none()
                            .p(px(6.))
                            .rounded(px(RADIUS_MD))
                            .cursor_pointer()
                            .hover(|s| s.bg(p.hover_wash(0.06)))
                            .child(icon("x", 12., p.muted_foreground))
                            .on_click(cx.listener(|this, _, _, cx| this.reset(cx))),
                    ),
            )
            .child(
                div()
                    .mt(px(18.))
                    .flex()
                    .items_center()
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
        stage_body()
            .child(waveform(&p, 0.22, true))
            .child(
                div()
                    .mt(px(24.))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(spinner("upload-spin", 15., p.foreground.opacity(0.7)))
                    .child(
                        div()
                            .text_size(px(TEXT_TITLE))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(p.foreground)
                            .child(t("upload.transcribing")),
                    ),
            )
            .when_some(name, |d, name| {
                d.child(
                    div()
                        .mt(px(4.))
                        .truncate()
                        .text_size(px(TEXT_BODY))
                        .text_color(p.muted_foreground)
                        .child(name),
                )
            })
            .child(
                div()
                    .mt(px(2.))
                    .text_size(px(TEXT_SMALL))
                    .line_height(px(LINE_SMALL))
                    .text_color(p.muted_foreground.opacity(0.7))
                    .child(t("upload.completionHint")),
            )
            .child(
                div().mt(px(18.)).flex().child(
                    Button::new("cancel-upload")
                        .label(t("upload.cancelTranscription"))
                        .variant(ButtonVariant::Outline)
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
        wide: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = cx.palette();
        let file_name = self.file.as_ref().map(|f| f.name.clone());
        let copy_text = text.clone();
        let status = match (&save_error, transcription_id) {
            (Some(err), _) => Some((err.clone(), p.destructive.opacity(0.8))),
            (None, Some(_)) => Some((t("upload.savedToHistory"), p.muted_foreground)),
            (None, None) => Some((
                t("controlPanel.history.dataRetentionDisabled"),
                p.muted_foreground,
            )),
        };
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .px(px(20.))
                    .py(px(14.))
                    .border_b_1()
                    .border_color(panel_border(&p))
                    .child(icon("check", 14., p.success))
                    .child(
                        div()
                            .flex_none()
                            .text_size(px(TEXT_BODY))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(p.foreground)
                            .child(t("upload.transcriptionComplete")),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(TEXT_SMALL))
                            .text_color(p.muted_foreground)
                            .when_some(file_name, |d, name| d.child(name)),
                    )
                    .when_some(status, |d, (status, color)| {
                        d.child(
                            div()
                                .flex_none()
                                .max_w(px(220.))
                                .truncate()
                                .text_size(px(TEXT_CAPTION))
                                .text_color(color)
                                .child(status),
                        )
                    }),
            )
            .child(
                div()
                    .id("upload-result")
                    .flex_1()
                    .min_h_0()
                    .when(!wide, |d| d.max_h(px(360.)))
                    .overflow_y_scroll()
                    .px(px(24.))
                    .py(px(20.))
                    .font_family(FONT_VOICE)
                    .text_size(px(TEXT_READING))
                    .line_height(px(LINE_READING))
                    .text_color(p.foreground)
                    .child(text),
            )
            .child(
                div()
                    .flex_none()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(8.))
                    .px(px(20.))
                    .py(px(14.))
                    .border_t_1()
                    .border_color(panel_border(&p))
                    .child(
                        Button::new("copy-full")
                            .label(t("upload.copyFullText"))
                            .icon("copy", 13.)
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
                    .child(div().flex_1())
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
        stage_body()
            .child(waveform(&p, 0.05, false))
            .child(
                div()
                    .mt(px(24.))
                    .flex()
                    .items_start()
                    .gap(px(10.))
                    .child(div().mt(px(2.)).child(icon(
                        "alert-circle",
                        15.,
                        p.destructive.opacity(0.8),
                    )))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(px(TEXT_BODY))
                            .line_height(px(20.))
                            .text_color(p.foreground)
                            .child(error),
                    ),
            )
            .child(
                div()
                    .mt(px(18.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        Button::new("upload-retry")
                            .label(t("upload.retry"))
                            .size(ButtonSize::Sm)
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
                            .on_click(cx.listener(|this, _, _, cx| this.reset(cx))),
                    ),
            )
            .into_any_element()
    }

    fn render_settings(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let p = cx.palette();
        let s = AppState::settings(cx);
        let model = s.remote_transcription_model.trim().to_string();
        let language = crate::text::language::languages()
            .iter()
            .find(|l| l.code == s.preferred_language)
            .map(|l| l.label.clone())
            .unwrap_or_else(|| s.preferred_language.clone());
        let row = |label: String, value: AnyElement| {
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap(px(12.))
                .px(px(16.))
                .py(px(5.))
                .child(
                    div()
                        .flex_none()
                        .text_size(px(TEXT_SMALL))
                        .text_color(p.muted_foreground)
                        .child(label),
                )
                .child(div().min_w_0().truncate().child(value))
        };
        let text_value = |value: String| {
            div()
                .truncate()
                .text_size(px(TEXT_SMALL))
                .text_color(p.foreground)
                .child(value)
                .into_any_element()
        };
        div()
            .id("upload-settings")
            .group("upload-settings")
            .flex_none()
            .pb(px(10.))
            .rounded(px(RADIUS_XL))
            .border_1()
            .border_color(panel_border(&p))
            .bg(p.card.opacity(0.4))
            .cursor_pointer()
            .hover(|s| s.bg(p.hover_wash(0.02)))
            .on_click(
                cx.listener(|_, _, _, cx| {
                    cx.emit(UploadEvent::OpenSettings("uploadTranscription"))
                }),
            )
            .child(side_header(
                &p,
                t("upload.settingsTitle"),
                Some(
                    div()
                        .text_size(px(TEXT_SMALL))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(p.muted_foreground)
                        .group_hover("upload-settings", |s| s.text_color(p.foreground))
                        .child(t("upload.changeModel"))
                        .into_any_element(),
                ),
            ))
            .child(row(
                t("upload.engine"),
                text_value(t("settings.transcription.modes.selfHosted")),
            ))
            .child(row(
                t("upload.model"),
                if model.is_empty() {
                    text_value("—".into())
                } else {
                    div()
                        .truncate()
                        .font_family(FONT_MONO)
                        .text_size(px(TEXT_CAPTION))
                        .text_color(p.foreground)
                        .child(model)
                        .into_any_element()
                },
            ))
            .child(row(t("upload.language"), text_value(language)))
            .into_any_element()
    }

    fn render_queue(
        &mut self,
        wide: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
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

        // Cancels a running batch, or clears a finished one.
        let cancels = self.batch_processing && !all_done;
        let action = (cancels || all_done).then(|| {
            div()
                .id("queue-action")
                .text_size(px(TEXT_CAPTION))
                .text_color(p.muted_foreground)
                .cursor_pointer()
                .hover(|s| s.text_color(p.foreground))
                .child(if cancels {
                    t("upload.cancelAll")
                } else {
                    t("upload.clearQueue")
                })
                .on_click(cx.listener(move |this, _, _, cx| {
                    if cancels {
                        this.cancel_batch(cx)
                    } else {
                        this.clear_queue(cx)
                    }
                }))
                .into_any_element()
        });

        let mut title = t("upload.queueTitle");
        if total > 0 {
            title = format!(
                "{title} · {}",
                t_with(
                    "upload.queueProgress",
                    &[
                        ("completed", &completed.to_string()),
                        ("total", &total.to_string()),
                    ],
                )
            );
        }

        let mut list = div()
            .id("batch-list")
            .when(wide, |d| d.flex_1().min_h_0())
            .when(!wide, |d| d.max_h(px(300.)))
            .overflow_y_scroll()
            .px(px(8.))
            .flex()
            .flex_col();
        for (index, item) in self.queue.iter().enumerate() {
            let status_icon = match item.status {
                ItemStatus::Done => icon("check", 12., p.success).into_any_element(),
                ItemStatus::Error => icon("x", 12., p.destructive.opacity(0.8)).into_any_element(),
                ItemStatus::Queued => icon("clock", 12., p.muted_foreground).into_any_element(),
                ItemStatus::Transcribing => {
                    spinner(("batch-spin", index), 12., p.foreground.opacity(0.7))
                        .into_any_element()
                }
            };
            let id = item.id.clone();
            let copy = item
                .text
                .clone()
                .filter(|_| item.transcription_id.is_none());
            let link = |id: (&'static str, usize), label: String| {
                div()
                    .id(id)
                    .flex_none()
                    .text_size(px(TEXT_CAPTION))
                    .text_color(p.muted_foreground)
                    .cursor_pointer()
                    .hover(|s| s.text_color(p.foreground))
                    .child(label)
            };
            list = list.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .px(px(8.))
                    .py(px(6.))
                    .rounded(px(RADIUS_MD))
                    .text_size(px(TEXT_SMALL))
                    .child(div().flex_none().child(status_icon))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_color(if item.status == ItemStatus::Queued {
                                p.muted_foreground
                            } else {
                                p.foreground
                            })
                            .child(item.name.clone()),
                    )
                    .when(
                        item.status == ItemStatus::Done && item.transcription_id.is_some(),
                        |d| {
                            d.child(
                                link(
                                    ("batch-history", index),
                                    t("controlPanel.history.sectionTitle"),
                                )
                                .on_click(
                                    cx.listener(|_, _, _, cx| cx.emit(UploadEvent::OpenHistory)),
                                ),
                            )
                        },
                    )
                    .when_some(copy, |d, text| {
                        d.child(
                            link(("batch-copy", index), t("controlPanel.history.copyText"))
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
                                    .flex_none()
                                    .max_w(px(110.))
                                    .truncate()
                                    .text_size(px(TEXT_CAPTION))
                                    .text_color(p.destructive.opacity(0.8))
                                    .child(err),
                            )
                        },
                    )
                    .when(item.status == ItemStatus::Queued, |d| {
                        d.child(
                            div()
                                .id(("batch-remove", index))
                                .flex_none()
                                .p(px(2.))
                                .cursor_pointer()
                                .child(icon("trash2", 11., p.muted_foreground))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.queue.retain(|i| i.id != id);
                                    cx.notify();
                                })),
                        )
                    }),
            );
        }

        div()
            .flex()
            .flex_col()
            .rounded(px(RADIUS_XL))
            .border_1()
            .border_color(panel_border(&p))
            .when(wide && total > 0, |d| d.flex_1().min_h_0())
            .child(side_header(&p, title, action))
            .when(failed > 0, |d| {
                d.child(
                    div()
                        .px(px(16.))
                        .pb(px(6.))
                        .text_size(px(TEXT_CAPTION))
                        .text_color(p.destructive.opacity(0.8))
                        .child(t_with("upload.queueFailed", &[("n", &failed.to_string())])),
                )
            })
            .when(total == 0, |d| {
                d.child(
                    div()
                        .px(px(16.))
                        .text_size(px(TEXT_SMALL))
                        .line_height(px(LINE_SMALL))
                        .text_color(p.muted_foreground.opacity(0.8))
                        .child(t("upload.queueEmpty")),
                )
            })
            .when(total > 0, |d| {
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
                .child(list)
            })
            .when(unsaved, |d| {
                d.child(
                    div()
                        .px(px(16.))
                        .pt(px(6.))
                        .text_size(px(TEXT_CAPTION))
                        .text_color(p.muted_foreground)
                        .child(t("controlPanel.history.dataRetentionDisabled")),
                )
            })
            .when(!self.batch_processing && has_queued, |d| {
                let disabled = !Self::provider_ready(cx) || self.stage == Stage::Transcribing;
                d.child(
                    div().px(px(12.)).pt(px(8.)).flex().child(
                        Button::new("batch-transcribe")
                            .label(t("upload.transcribe"))
                            .size(ButtonSize::Sm)
                            .full_width()
                            .disabled(disabled)
                            .on_click(cx.listener(|this, _, _, cx| this.process_queue(cx))),
                    ),
                )
            })
            .child(div().flex_none().h(px(12.)))
            .into_any_element()
    }
}

impl Render for UploadView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let viewport = window.viewport_size();
        let wide = viewport.width >= px(WIDE_MIN_WIDTH);
        let stage = self.render_stage_panel(wide, window, cx);
        let settings = self.render_settings(cx);
        let queue = self.render_queue(wide, window, cx);

        div()
            .w_full()
            .flex()
            .gap(px(16.))
            .p(px(20.))
            .when(wide, |d| d.flex_row().h(viewport.height - px(66.)))
            .when(!wide, |d| d.flex_col().min_h(viewport.height - px(66.)))
            .child(stage)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(12.))
                    .when(wide, |d| {
                        d.flex_grow(1.)
                            .flex_basis(px(0.))
                            .min_w(px(260.))
                            .max_w(px(360.))
                    })
                    .when(!wide, |d| d.w_full())
                    .child(settings)
                    .child(queue),
            )
    }
}

/// Window width at which the side column moves beside the stage panel.
const WIDE_MIN_WIDTH: f32 = 860.;

/// Hover and drag styles on the stage panel's children key off this group.
const STAGE_GROUP: &str = "upload-stage";

/// Height of the decorative waveform, in px.
const WAVE_HEIGHT: f32 = 96.;

/// Bar heights of the decorative waveform as fractions of `WAVE_HEIGHT`.
/// Fixed so the motif stays put between renders.
const WAVE: [f32; 56] = [
    0.25, 0.34, 0.22, 0.18, 0.24, 0.43, 0.26, 0.33, 0.54, 0.37, 0.52, 0.65, 0.73, 0.54, 0.44, 0.54,
    0.55, 0.64, 0.47, 0.81, 0.4, 0.82, 0.36, 0.54, 0.63, 0.7, 0.73, 0.78, 0.83, 0.59, 0.89, 0.37,
    0.38, 0.59, 0.87, 0.38, 0.79, 0.69, 0.8, 0.51, 0.71, 0.7, 0.58, 0.59, 0.39, 0.64, 0.56, 0.55,
    0.45, 0.36, 0.43, 0.25, 0.21, 0.27, 0.18, 0.19,
];

fn panel_border(p: &Palette) -> Hsla {
    p.border.opacity(if p.dark { 1. } else { 0.8 })
}

/// The stage panel's padded column: the waveform fills the space above and
/// the stage's text and actions sit at the bottom left.
fn stage_body() -> Div {
    div().flex_1().flex().flex_col().p(px(28.))
}

/// A static waveform, or a rolling one while `animated`. `tone` is the bar
/// opacity; bars brighten while files are dragged over the stage panel.
fn waveform(p: &Palette, tone: f32, animated: bool) -> Div {
    let rest = p.foreground.opacity(tone);
    let lit = p.foreground.opacity(0.4);
    let mut bars = div()
        .w_full()
        .h(px(WAVE_HEIGHT))
        .flex()
        .items_center()
        .gap(px(3.));
    for (i, level) in WAVE.iter().copied().enumerate() {
        let height = (WAVE_HEIGHT * level).max(4.);
        let bar = div()
            .flex_1()
            .h(px(height))
            .rounded(px(1.5))
            .bg(rest)
            .group_drag_over::<ExternalPaths>(STAGE_GROUP, move |s| s.bg(lit));
        bars = bars.child(if animated {
            let offset = i as f32 / WAVE.len() as f32;
            bar.with_animation(
                ("upload-wave", i),
                Animation::new(Duration::from_millis(1600)).repeat(),
                move |d, t| {
                    let k = 0.5 - 0.5 * ((t - offset) * TAU).cos();
                    d.h(px(height * (0.45 + 0.55 * k)))
                },
            )
            .into_any_element()
        } else {
            bar.into_any_element()
        });
    }
    div()
        .flex_1()
        .min_h(px(WAVE_HEIGHT))
        .flex()
        .items_center()
        .child(bars)
}

fn file_badge(p: &Palette) -> Div {
    div()
        .size(px(36.))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(RADIUS_MD))
        .border_1()
        .border_color(panel_border(p))
        .bg(p.card)
        .child(icon("file-audio", 16., p.muted_foreground))
}

/// Title row of a side-column card, with an optional trailing control.
fn side_header(p: &Palette, title: String, trailing: Option<AnyElement>) -> Div {
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(8.))
        .px(px(16.))
        .pt(px(12.))
        .pb(px(8.))
        .child(
            div()
                .min_w_0()
                .truncate()
                .text_size(px(TEXT_SMALL))
                .font_weight(FontWeight::MEDIUM)
                .text_color(p.muted_foreground)
                .child(title),
        )
        .children(trailing)
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
