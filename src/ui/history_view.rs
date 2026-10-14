//! Home: the local History grouped by day.

use std::collections::HashSet;
use std::time::Duration;

use chrono::{DateTime, Datelike, Local, NaiveDate, Utc};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use whisper::db::{TranscriptionItem, TranscriptionStatus};
use whisper::i18n::{self, Language, t, t_with};

use crate::app_state::{AppEvent, AppState};
use crate::backend;

use super::components::{f32_tx, icon, kbd, spinner};
use super::overlays::{DialogRequest, Toast, ToastVariant, show_dialog, show_toast};
use super::theme::{
    LINE_READING, LINE_SMALL, Palette, PaletteExt, TEXT_BODY, TEXT_CAPTION, TEXT_READING,
    TEXT_SMALL, TEXT_TITLE, ease_css,
};

pub enum HistoryEvent {
    OpenSettings(&'static str),
}

pub struct HistoryView {
    /// A row picked in the search palette flashes briefly so it is easy to find.
    highlighted: Option<(i64, std::time::Instant)>,
    expanded: HashSet<i64>,
    retrying: HashSet<i64>,
    open_menu: Option<i64>,
    hovering_list: bool,
}

impl EventEmitter<HistoryEvent> for HistoryView {}

pub fn copy_with_toast(text: String, cx: &mut App) {
    cx.write_to_clipboard(ClipboardItem::new_string(text));
    show_toast(
        Toast::new(t("controlPanel.history.copiedTitle"))
            .description(t("controlPanel.history.copiedDescription"))
            .variant(ToastVariant::Success)
            .duration(Duration::from_millis(2000)),
        cx,
    );
}

fn local_time(item: &TranscriptionItem) -> Option<DateTime<Local>> {
    item.occurred_at()
        .map(|t: DateTime<Utc>| t.with_timezone(&Local))
}

fn format_time(item: &TranscriptionItem) -> String {
    let Some(time) = local_time(item) else {
        return String::new();
    };
    match i18n::language() {
        Language::ZhCn => time.format("%H:%M").to_string(),
        Language::En => time.format("%I:%M %p").to_string(),
    }
}

pub fn format_date_group(date: NaiveDate) -> String {
    let today = Local::now().date_naive();
    if date == today {
        return t("controlPanel.history.dateGroups.today");
    }
    if Some(date) == today.pred_opt() {
        return t("controlPanel.history.dateGroups.yesterday");
    }
    match i18n::language() {
        Language::ZhCn => format!("{}年{}月{}日", date.year(), date.month(), date.day()),
        Language::En => date.format("%b %-d, %Y").to_string(),
    }
}

fn format_mm_ss(total_seconds: i64) -> String {
    format!("{}:{:02}", total_seconds / 60, total_seconds % 60)
}

impl HistoryView {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let state = AppState::global(cx);
        cx.subscribe(&state, |_, _, event: &AppEvent, cx| {
            if matches!(event, AppEvent::HistoryChanged | AppEvent::SettingsChanged) {
                cx.notify();
            }
        })
        .detach();
        if !state.read(cx).history_loaded {
            Self::reload(false, cx);
        }
        Self {
            highlighted: None,
            expanded: HashSet::new(),
            retrying: HashSet::new(),
            open_menu: None,
            hovering_list: false,
        }
    }

    pub fn highlight(&mut self, id: i64, cx: &mut Context<Self>) {
        self.highlighted = Some((id, std::time::Instant::now()));
        cx.notify();
    }

    fn reload(include_discarded: bool, cx: &mut App) {
        let result = AppState::global(cx).update(cx, |s, cx| s.load_history(include_discarded, cx));
        if result.is_err() {
            show_dialog(
                DialogRequest::alert(
                    t("controlPanel.history.couldNotLoadTitle"),
                    Some(t("controlPanel.history.couldNotLoadDescription").into()),
                ),
                cx,
            );
        }
    }

    fn delete(id: i64, cx: &mut App) {
        let mut request = DialogRequest::confirm(
            t("controlPanel.history.deleteTitle"),
            Some(t("controlPanel.history.deleteDescription").into()),
            true,
            move |_, cx| {
                let result =
                    AppState::global(cx).update(cx, |s, cx| s.delete_transcription(id, cx));
                match result {
                    Ok(true) => {}
                    Ok(false) => show_dialog(
                        DialogRequest::alert(
                            t("controlPanel.history.couldNotDeleteTitle"),
                            Some(t("controlPanel.history.couldNotDeleteDescription").into()),
                        ),
                        cx,
                    ),
                    Err(_) => show_dialog(
                        DialogRequest::alert(
                            t("controlPanel.history.couldNotDeleteTitle"),
                            Some(t("controlPanel.history.couldNotDeleteDescriptionGeneric").into()),
                        ),
                        cx,
                    ),
                }
            },
        );
        request.confirm_label = t("controlPanel.history.deleteItem").into();
        show_dialog(request, cx);
    }

    fn clear_all(cx: &mut App) {
        let request = DialogRequest::confirm(
            t("controlPanel.history.clearAllTitle"),
            Some(t("controlPanel.history.clearAllDescriptionDevice").into()),
            true,
            |_, cx| {
                let result = AppState::global(cx).update(cx, |s, cx| s.clear_transcriptions(cx));
                if result.is_ok() {
                    show_toast(
                        Toast::new(t("controlPanel.history.clearAllSuccess"))
                            .variant(ToastVariant::Success)
                            .duration(Duration::from_millis(2000)),
                        cx,
                    );
                } else {
                    show_dialog(
                        DialogRequest::alert(
                            t("controlPanel.history.clearAllErrorTitle"),
                            Some(t("controlPanel.history.clearAllErrorDescription").into()),
                        ),
                        cx,
                    );
                }
            },
        );
        show_dialog(request, cx);
    }

    fn show_in_folder(id: i64, cx: &mut App) {
        let path = AppState::global(cx).read(cx).audio.audio_path(id);
        match path {
            Some(path) => backend::reveal_in_finder(&path),
            None => show_toast(
                Toast::new(t("controlPanel.history.audioNotFound"))
                    .variant(ToastVariant::Destructive),
                cx,
            ),
        }
    }

    fn retry(&mut self, id: i64, recover: bool, cx: &mut Context<Self>) {
        if !self.retrying.insert(id) {
            return;
        }
        self.open_menu = None;
        cx.notify();
        let task = crate::dictation::retry_transcription(id, cx);
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.retrying.remove(&id);
                match result {
                    Ok(()) => show_toast(
                        Toast::new(if recover {
                            t("controlPanel.history.discarded.recovered")
                        } else {
                            t("controlPanel.history.retrySuccess")
                        }),
                        cx,
                    ),
                    Err(message) => show_toast(
                        Toast::new(t("controlPanel.history.retryError"))
                            .description(message)
                            .variant(ToastVariant::Destructive),
                        cx,
                    ),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn render_discarded_toggle(&self, include: bool, p: &Palette) -> Stateful<Div> {
        let p = *p;
        div()
            .id("toggle-discarded")
            .flex()
            .items_center()
            .gap(px(4.))
            .px(px(6.))
            .py(px(2.))
            .rounded(px(4.))
            .text_size(px(TEXT_CAPTION))
            .text_color(p.muted_foreground.opacity(0.7))
            .cursor_pointer()
            .hover(move |s| s.text_color(p.foreground).bg(p.hover_wash(0.05)))
            .child(icon("archive", 11., p.muted_foreground.opacity(0.7)))
            .child(if include {
                t("controlPanel.history.discarded.hide")
            } else {
                t("controlPanel.history.discarded.show")
            })
            .on_click(move |_, _, cx| Self::reload(!include, cx))
    }

    fn render_item(
        &self,
        item: &TranscriptionItem,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = cx.palette();
        let id = item.id;
        let is_failed = item.status == TranscriptionStatus::Failed;
        let is_discarded = item.status == TranscriptionStatus::Discarded;
        let is_transcribed = !is_failed && !is_discarded;
        let has_audio = item.has_audio;
        let raw_text = item.raw_text.clone();
        let expanded = self.expanded.contains(&id);
        let retrying = self.retrying.contains(&id);
        let menu_open = self.open_menu == Some(id);
        let group: SharedString = format!("row-{id}").into();

        // Raw transcript panel: grid-rows 0fr→1fr over 200 ms.
        let raw_progress = f32_tx(
            ("raw-expand", id as usize),
            if expanded { 1. } else { 0. },
            200,
            ease_css,
            window,
            cx,
        );

        let body: AnyElement = if is_failed {
            let code = item.error_code.clone().unwrap_or_default();
            let is_config = matches!(
                code.as_str(),
                "API_KEY_MISSING"
                    | "INVALID_KEY"
                    | "MODEL_NOT_AVAILABLE"
                    | "CUSTOM_ENDPOINT_INVALID"
            );
            div()
                .flex()
                .items_start()
                .gap(px(8.))
                .child(
                    div()
                        .mt(px(2.))
                        .child(icon("alert-circle", 14., p.destructive)),
                )
                .child(
                    div()
                        .min_w_0()
                        .child(
                            div()
                                .text_size(px(TEXT_BODY))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(p.destructive)
                                .child(t("controlPanel.history.transcriptionFailed")),
                        )
                        .when_some(
                            item.error_message.clone().filter(|m| !m.is_empty()),
                            |d, msg| {
                                d.child(
                                    div()
                                        .mt(px(2.))
                                        .text_size(px(TEXT_SMALL))
                                        .line_height(px(LINE_SMALL))
                                        .text_color(p.muted_foreground)
                                        .child(msg),
                                )
                            },
                        )
                        .when(is_config, |d| {
                            d.child(
                                div()
                                    .mt(px(4.))
                                    .flex()
                                    .flex_wrap()
                                    .gap(px(4.))
                                    .text_size(px(TEXT_SMALL))
                                    .text_color(p.muted_foreground)
                                    .child(
                                        div()
                                            .id(("failed-cta", id as usize))
                                            .text_color(p.primary)
                                            .cursor_pointer()
                                            .hover(|s| s.underline())
                                            .child(if has_audio {
                                                t("controlPanel.history.failedCtaSettings")
                                            } else {
                                                t("controlPanel.history.failedCtaSettingsOnly")
                                            })
                                            .on_click(cx.listener(|_, _, _, cx| {
                                                cx.emit(HistoryEvent::OpenSettings("transcription"))
                                            })),
                                    )
                                    .when(has_audio, |d| {
                                        d.child(t("controlPanel.history.failedCtaAndRetry"))
                                    }),
                            )
                        }),
                )
                .into_any_element()
        } else if is_discarded {
            let label = match item.audio_duration_ms.filter(|ms| *ms > 0) {
                Some(ms) => t_with(
                    "controlPanel.history.discarded.recordingWithDuration",
                    &[(
                        "duration",
                        &format_mm_ss((ms as f64 / 1000.).round() as i64),
                    )],
                ),
                None => t("controlPanel.history.discarded.recording"),
            };
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(
                    div()
                        .px(px(6.))
                        .py(px(2.))
                        .rounded(px(2.))
                        .bg(p.muted)
                        .text_size(px(TEXT_CAPTION))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(p.muted_foreground)
                        .child(t("controlPanel.history.discarded.badge").to_uppercase()),
                )
                .child(
                    div()
                        .truncate()
                        .text_size(px(TEXT_BODY))
                        .text_color(p.muted_foreground)
                        .child(label),
                )
                .into_any_element()
        } else {
            div()
                .text_size(px(TEXT_READING))
                .line_height(px(LINE_READING))
                .text_color(p.foreground)
                .child(item.text.clone())
                .into_any_element()
        };

        let raw_section =
            (is_transcribed && raw_text.is_some() && raw_progress > 0.001).then(|| {
                let raw = raw_text.clone().unwrap_or_default();
                let same = raw == item.text;
                let copy_raw = raw.clone();
                div()
                    .overflow_hidden()
                    .opacity(raw_progress)
                    .max_h(px(400. * raw_progress))
                    .child(
                        div()
                            .mt(px(8.))
                            .pt(px(8.))
                            .border_t_1()
                            .border_color(p.border.opacity(0.7))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .child(
                                        div()
                                            .text_size(px(TEXT_CAPTION))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(p.muted_foreground)
                                            .child(
                                                t("controlPanel.history.rawTranscript")
                                                    .to_uppercase(),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .id(("copy-raw", id as usize))
                                            .size(px(20.))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .rounded(px(2.))
                                            .cursor_pointer()
                                            .hover(|s| s.bg(p.foreground.opacity(0.1)))
                                            .child(icon("copy", 10., p.muted_foreground))
                                            .on_click(move |_, _, cx| {
                                                copy_with_toast(copy_raw.clone(), cx)
                                            }),
                                    ),
                            )
                            .child(
                                div()
                                    .mt(px(4.))
                                    .text_size(px(TEXT_SMALL))
                                    .line_height(px(LINE_SMALL))
                                    .text_color(p.muted_foreground.opacity(0.8))
                                    .child(raw),
                            )
                            .when(same, |d| {
                                d.child(
                                    div()
                                        .mt(px(4.))
                                        .text_size(px(TEXT_CAPTION))
                                        .italic()
                                        .text_color(p.muted_foreground.opacity(0.7))
                                        .child(t("controlPanel.history.noAiProcessing")),
                                )
                            }),
                    )
            });

        let action_btn = |id_str: &'static str, name: &'static str| {
            div()
                .id((id_str, id as usize))
                .size(px(28.))
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .cursor_pointer()
                .hover(move |s| s.bg(p.hover_wash(0.06)))
                .child(icon(name, 13., p.muted_foreground.opacity(0.7)))
        };

        let copy_text = item.text.clone();
        let actions = div()
            .flex_none()
            .flex()
            .items_start()
            .gap(px(4.))
            .mt(px(-2.))
            .mr(px(-6.))
            .when(is_transcribed && !menu_open, |d| {
                d.opacity(0.).group_hover(group.clone(), |s| s.opacity(1.))
            })
            .when(is_transcribed, |d| {
                d.child(
                    action_btn("copy-row", "copy")
                        .on_click(move |_, _, cx| copy_with_toast(copy_text.clone(), cx)),
                )
            })
            .child(
                div()
                    .relative()
                    .child(if retrying {
                        div()
                            .size(px(28.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(spinner(
                                ("retry-spin", id as usize),
                                13.,
                                p.muted_foreground,
                            ))
                            .into_any_element()
                    } else {
                        action_btn("more-row", "more-vertical")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.open_menu = if this.open_menu == Some(id) {
                                    None
                                } else {
                                    Some(id)
                                };
                                cx.notify();
                            }))
                            .into_any_element()
                    })
                    .when(menu_open, |d| d.child(self.render_menu(item, cx))),
            );

        let route_translation = item.route_kind.as_deref() == Some("translation");
        let flash = self
            .highlighted
            .filter(|(hid, at)| *hid == id && at.elapsed() < Duration::from_millis(1600))
            .map(|(_, at)| {
                window.request_animation_frame();
                let t = at.elapsed().as_secs_f32() / 1.6;
                (1. - t).clamp(0., 1.)
            });
        div()
            .id(("history-row", id as usize))
            .when_some(flash, |d, f| d.bg(p.primary.opacity(0.12 * f)))
            .group(group)
            .flex()
            .gap(px(16.))
            .rounded(px(12.))
            .px(px(12.))
            .py(px(10.))
            .when(is_failed, |d| d.bg(p.destructive.opacity(0.05)))
            .when(is_discarded, |d| d.bg(p.muted.opacity(0.2)).opacity(0.8))
            .when(is_transcribed, |d| {
                d.hover(move |s| {
                    s.bg(if p.dark {
                        p.white.opacity(0.03)
                    } else {
                        p.muted.opacity(0.3)
                    })
                })
            })
            .when(route_translation, |d| {
                d.border_l_2().border_color(p.primary.opacity(0.7))
            })
            .child(
                div()
                    .w(px(64.))
                    .flex_none()
                    .pt(px(4.))
                    .text_size(px(TEXT_SMALL))
                    .text_color(p.muted_foreground.opacity(0.7))
                    .child(format_time(item)),
            )
            .child(div().flex_1().min_w_0().child(body).children(raw_section))
            .child(actions)
            .into_any_element()
    }

    fn render_menu(&self, item: &TranscriptionItem, cx: &mut Context<Self>) -> AnyElement {
        let p = cx.palette();
        let id = item.id;
        let is_failed = item.status == TranscriptionStatus::Failed;
        let is_discarded = item.status == TranscriptionStatus::Discarded;
        let is_transcribed = !is_failed && !is_discarded;
        let mut entries: Vec<(&'static str, String, u8, bool)> = Vec::new();
        if is_discarded && item.has_audio {
            entries.push((
                "archive-restore",
                t("controlPanel.history.discarded.recover"),
                0,
                false,
            ));
        }
        if !is_discarded && item.has_audio {
            let label = if item.route_kind.as_deref() == Some("translation") {
                t("controlPanel.history.retryTranslationMode")
            } else {
                t("controlPanel.history.retryTranscription")
            };
            entries.push(("rotate-ccw", label, 1, false));
        }
        if is_transcribed && item.raw_text.is_some() {
            entries.push((
                "file-text",
                t("controlPanel.history.viewRawTranscript"),
                2,
                false,
            ));
        }
        if item.has_audio {
            entries.push((
                "folder-open",
                t("controlPanel.history.showInFolder"),
                3,
                false,
            ));
        }
        entries.push(("trash2", t("controlPanel.history.deleteItem"), 4, true));

        let mut menu = div()
            .id(("row-menu", id as usize))
            .occlude()
            .mt(px(4.))
            .min_w(px(180.))
            .p(px(4.))
            .flex()
            .flex_col()
            .rounded(px(8.))
            .border_1()
            .border_color(if p.dark {
                p.border_hover
            } else {
                p.border.opacity(0.7)
            })
            .bg(if p.dark { p.surface_3 } else { p.popover })
            .shadow_lg()
            .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                this.open_menu = None;
                cx.notify();
            }));
        for (icon_name, label, action, destructive) in entries {
            let color = if destructive {
                p.destructive
            } else {
                p.foreground
            };
            menu = menu.child(
                div()
                    .id(("row-menu-item", action as usize))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .px(px(8.))
                    .py(px(6.))
                    .rounded(px(6.))
                    .text_size(px(TEXT_BODY))
                    .text_color(color)
                    .cursor_pointer()
                    .hover(move |s| {
                        s.bg(if destructive {
                            p.destructive.opacity(0.08)
                        } else if p.dark {
                            p.white.opacity(0.06)
                        } else {
                            p.muted
                        })
                    })
                    .child(icon(icon_name, 14., color.opacity(0.7)))
                    .child(label)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.open_menu = None;
                        match action {
                            0 => this.retry(id, true, cx),
                            1 => this.retry(id, false, cx),
                            2 => {
                                if !this.expanded.remove(&id) {
                                    this.expanded.insert(id);
                                }
                            }
                            3 => Self::show_in_folder(id, cx),
                            _ => Self::delete(id, cx),
                        }
                        cx.notify();
                    })),
            );
        }
        deferred(
            anchored()
                .anchor(Anchor::TopRight)
                .snap_to_window_with_margin(px(8.))
                .child(menu),
        )
        .with_priority(1)
        .into_any_element()
    }
}

impl Render for HistoryView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = cx.palette();
        let state = AppState::global(cx);
        let (history, loaded, include, retention, hotkey) = {
            let s = state.read(cx);
            (
                s.history.clone(),
                s.history_loaded,
                s.include_discarded,
                s.settings.data_retention_enabled,
                s.settings.effective_dictation_key(),
            )
        };

        let mut content = div().min_w_0().flex_1();
        if !retention {
            content = content.child(
                div()
                    .mb(px(12.))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .px(px(14.))
                    .py(px(10.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(p.amber_border)
                    .bg(p.amber_bg)
                    .child(
                        div()
                            .text_size(px(TEXT_BODY))
                            .text_color(p.amber_icon)
                            .child("⊘"),
                    )
                    .child(
                        div()
                            .text_size(px(TEXT_SMALL))
                            .line_height(px(LINE_SMALL))
                            .text_color(p.amber_text)
                            .child(t("controlPanel.history.dataRetentionDisabled")),
                    ),
            );
        }

        if !loaded && history.is_empty() {
            content = content.child(
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .gap(px(8.))
                    .py(px(40.))
                    .child(spinner("history-loading", 14., p.primary))
                    .child(
                        div()
                            .text_size(px(TEXT_BODY))
                            .text_color(p.muted_foreground)
                            .child(t("controlPanel.loading")),
                    ),
            );
        } else if history.is_empty() {
            content = content.child(render_empty_with_hotkey(self, include, &hotkey, &p));
        } else {
            // Group consecutive rows by local day.
            let mut groups: Vec<(String, Vec<TranscriptionItem>)> = Vec::new();
            for item in history {
                let label = local_time(&item)
                    .map(|t| format_date_group(t.date_naive()))
                    .unwrap_or_default();
                match groups.last_mut() {
                    Some((last, items)) if *last == label => items.push(item),
                    _ => groups.push((label, vec![item])),
                }
            }
            let mut list =
                div()
                    .id("history-list")
                    .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                        this.hovering_list = *hovered;
                        cx.notify();
                    }));
            let show_controls = self.hovering_list;
            for (index, (label, items)) in groups.into_iter().enumerate() {
                let mut header = div()
                    .relative()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .pt(px(8.))
                    .pb(px(8.))
                    .bg(p.background)
                    .child(
                        div()
                            .flex_none()
                            .text_size(px(TEXT_SMALL))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(if p.dark {
                                p.foreground.opacity(0.75)
                            } else {
                                p.muted_foreground
                            })
                            .child(label),
                    )
                    .child(div().h(px(1.)).flex_1().bg(p.border.opacity(0.7)));
                if index == 0 {
                    header = header.child(
                        div()
                            .absolute()
                            .right(px(0.))
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .pl(px(8.))
                            .bg(p.background)
                            .opacity(if show_controls { 1. } else { 0. })
                            .child(self.render_discarded_toggle(include, &p))
                            .child(
                                div()
                                    .id("clear-all")
                                    .flex()
                                    .items_center()
                                    .gap(px(4.))
                                    .px(px(6.))
                                    .py(px(2.))
                                    .rounded(px(4.))
                                    .text_size(px(TEXT_CAPTION))
                                    .text_color(p.muted_foreground.opacity(0.7))
                                    .cursor_pointer()
                                    .hover(move |s| {
                                        s.text_color(p.destructive).bg(p.destructive.opacity(0.08))
                                    })
                                    .child(icon("trash2", 11., p.muted_foreground.opacity(0.7)))
                                    .child(t("controlPanel.history.clearAll"))
                                    .on_click(|_, _, cx| Self::clear_all(cx)),
                            ),
                    );
                }
                let mut rows = div().flex().flex_col().gap(px(2.)).mx(px(-12.));
                for item in &items {
                    rows = rows.child(self.render_item(item, window, cx));
                }
                list = list.child(
                    div()
                        .when(index > 0, |d| d.mt(px(32.)))
                        .child(header)
                        .child(rows),
                );
            }
            content = content.child(list);
        }

        div()
            .px(px(16.))
            .pt(px(16.))
            .pb(px(24.))
            .child(div().mx_auto().max_w(px(1024.)).flex().child(content))
    }
}

fn render_empty_with_hotkey(
    view: &HistoryView,
    include: bool,
    hotkey: &str,
    p: &Palette,
) -> AnyElement {
    let mut keys = div().flex().items_center().gap(px(4.));
    for (i, key) in backend::parse_hotkey_list(hotkey).into_iter().enumerate() {
        if i > 0 {
            keys = keys.child(div().text_color(p.foreground.opacity(0.45)).child("/"));
        }
        keys = keys.child(kbd(backend::format_hotkey_label(&key), p));
    }
    empty_card(view, include, keys, p)
}

fn empty_card(view: &HistoryView, include: bool, keys: Div, p: &Palette) -> AnyElement {
    div()
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .pt(px(8.))
                .pb(px(10.))
                .child(
                    div()
                        .text_size(px(TEXT_BODY))
                        .text_color(p.muted_foreground)
                        .child(t("controlPanel.history.sectionTitle")),
                )
                .child(view.render_discarded_toggle(include, p)),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .items_center()
                .rounded(px(16.))
                .border_1()
                .border_color(if p.dark {
                    p.white.opacity(0.1)
                } else {
                    p.border.opacity(0.7)
                })
                .bg(if p.dark {
                    p.surface_2.opacity(0.6)
                } else {
                    p.card.opacity(0.5)
                })
                .px(px(24.))
                .py(px(40.))
                .child(
                    div()
                        .size(px(44.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(12.))
                        .bg(p.surface_3)
                        .child(icon("mic", 20., p.foreground.opacity(0.7))),
                )
                .child(
                    div()
                        .mt(px(16.))
                        .text_size(px(TEXT_TITLE))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(p.foreground)
                        .child(t("controlPanel.history.empty")),
                )
                .child(
                    div()
                        .mt(px(4.))
                        .max_w(px(320.))
                        .text_center()
                        .text_size(px(TEXT_BODY))
                        .line_height(px(21.))
                        .text_color(p.foreground.opacity(0.7))
                        .child(t("controlPanel.history.emptyDescription")),
                )
                .child(
                    div()
                        .mt(px(20.))
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap(px(8.))
                        .child(
                            div()
                                .w(px(224.))
                                .mb(px(4.))
                                .flex()
                                .flex_col()
                                .gap(px(8.))
                                .children([1.0f32, 0.8, 0.6].map(|w| {
                                    div().h(px(8.)).w(relative(w)).rounded_full().bg(if p.dark {
                                        p.white.opacity(0.08)
                                    } else {
                                        p.foreground.opacity(0.06)
                                    })
                                })),
                        )
                        .child(
                            div()
                                .h(px(30.))
                                .flex()
                                .items_center()
                                .gap(px(6.))
                                .px(px(12.))
                                .rounded_full()
                                .bg(p.surface_3)
                                .text_size(px(TEXT_SMALL))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(p.foreground.opacity(0.7))
                                .child(t("controlPanel.history.press"))
                                .child(keys)
                                .child(t("controlPanel.history.toStart")),
                        ),
                ),
        )
        .into_any_element()
}
