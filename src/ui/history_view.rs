//! Home: the local History grouped by day.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use chrono::{DateTime, Datelike, Local, NaiveDate, Utc};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::db::TranscriptionItem;
use crate::i18n::{self, Language, t};

use crate::app_state::{AppEvent, AppState};
use crate::backend;

use super::components::{f32_tx, icon, kbd, shimmer_text, skeleton_line, spinner};
use super::overlays::{DialogRequest, Toast, ToastVariant, show_dialog, show_toast};
use super::theme::{
    DUR_BASE, DUR_FAST, DUR_SHEET, DUR_SNAP, FONT_VOICE, LINE_READING, LINE_SMALL, Palette,
    PaletteExt, READING_MAX_WIDTH, TEXT_BODY, TEXT_CAPTION, TEXT_READING, TEXT_SMALL, TEXT_TITLE,
    ease_out, ease_snap, progress,
};

/// How long a new or picked row stays tinted.
const FLASH: Duration = Duration::from_millis(1600);

/// The placeholder row shown while a Dictation is transcribing.
struct PendingRow {
    /// The top row when transcription started; a new top row replaces the placeholder.
    base_top: Option<i64>,
    since: Instant,
    arrived: bool,
}

pub struct HistoryView {
    /// A row picked in the search palette flashes briefly so it is easy to find.
    highlighted: Option<(i64, Instant)>,
    expanded: HashSet<i64>,
    retrying: HashSet<i64>,
    open_menu: Option<i64>,
    hovering_list: bool,
    hovered_row: Option<i64>,
    pending: Option<PendingRow>,
    /// A row that just landed from Dictation; it reveals and tints briefly.
    arrived: Option<(i64, Instant)>,
}

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

impl HistoryView {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let state = AppState::global(cx);
        // Dictation phase changes drive the transcribing placeholder. Tracked
        // here rather than in render so it starts before the transcript is
        // saved even while Home is not showing.
        cx.observe(&state, |this, state, cx| {
            let s = state.read(cx);
            let busy = s.settings.data_retention_enabled && s.dictation.is_processing;
            let top = s.history.first().map(|item| item.id);
            this.sync_pending(busy, top);
            cx.notify();
        })
        .detach();
        cx.subscribe(&state, |_, _, event: &AppEvent, cx| {
            if matches!(event, AppEvent::HistoryChanged | AppEvent::SettingsChanged) {
                cx.notify();
            }
        })
        .detach();
        if !state.read(cx).history_loaded {
            Self::reload(cx);
        }
        Self {
            highlighted: None,
            expanded: HashSet::new(),
            retrying: HashSet::new(),
            open_menu: None,
            hovering_list: false,
            hovered_row: None,
            pending: None,
            arrived: None,
        }
    }

    pub fn highlight(&mut self, id: i64, cx: &mut Context<Self>) {
        self.highlighted = Some((id, Instant::now()));
        cx.notify();
    }

    fn reload(cx: &mut App) {
        let result = AppState::global(cx).update(cx, |s, cx| s.load_history(cx));
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

    fn retry(&mut self, id: i64, cx: &mut Context<Self>) {
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
                    Ok(()) => show_toast(Toast::new(t("controlPanel.history.retrySuccess")), cx),
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

    fn render_item(
        &self,
        item: &TranscriptionItem,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = cx.palette();
        let id = item.id;
        let raw_text = item.raw_text.clone();
        let expanded = self.expanded.contains(&id);
        let retrying = self.retrying.contains(&id);
        let menu_open = self.open_menu == Some(id);
        let arrived = self
            .arrived
            .filter(|(aid, at)| *aid == id && at.elapsed() < FLASH && !cx.reduce_motion())
            .map(|(_, at)| at.elapsed());

        // Raw transcript panel: grid-rows 0fr→1fr over 200 ms.
        let raw_progress = f32_tx(
            ("raw-expand", id as usize),
            if expanded { 1. } else { 0. },
            DUR_BASE,
            ease_snap,
            window,
            cx,
        );

        // A new transcript eases up 3 px into place.
        let reveal = arrived.map_or(1., |e| ease_out(progress(e, DUR_BASE)));
        let body = div()
            .relative()
            .top(px(3. * (1. - reveal)))
            .opacity(reveal)
            .font_family(FONT_VOICE)
            .text_size(px(TEXT_READING))
            .line_height(px(LINE_READING))
            .text_color(p.foreground)
            .child(item.text.clone())
            .into_any_element();

        let raw_section = (raw_text.is_some() && raw_progress > 0.001).then(|| {
            let raw = raw_text.clone().unwrap_or_default();
            let same = raw == item.text;
            let copy_raw = raw.clone();
            div()
                .overflow_hidden()
                .opacity(raw_progress)
                .max_h(px(400. * raw_progress))
                .child(
                    div()
                        .relative()
                        .top(px(3. * (1. - raw_progress)))
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
                                            t("controlPanel.history.rawTranscript").to_uppercase(),
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
                                .font_family(FONT_VOICE)
                                .text_size(px(TEXT_BODY))
                                .line_height(px(20.))
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

        // Row actions surface on hover in 120 ms and leave in 60 ms.
        let actions_shown = menu_open || self.hovered_row == Some(id);
        let actions_t = f32_tx(
            ("row-actions", id as usize),
            if actions_shown { 1. } else { 0. },
            if actions_shown { DUR_SNAP } else { DUR_FAST },
            ease_snap,
            window,
            cx,
        );
        let copy_text = item.text.clone();
        let actions = div()
            .flex_none()
            .flex()
            .items_start()
            .gap(px(4.))
            .relative()
            .top(px(2. * (1. - actions_t)))
            .mt(px(-2.))
            .mr(px(-6.))
            .opacity(actions_t)
            .child(
                action_btn("copy-row", "copy")
                    .on_click(move |_, _, cx| copy_with_toast(copy_text.clone(), cx)),
            )
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
        let picked = self
            .highlighted
            .filter(|(hid, at)| *hid == id && at.elapsed() < FLASH)
            .map(|(_, at)| 0.12 * (1. - progress(at.elapsed(), 1600)));
        // A new transcript tints in and eases out over the flash.
        let landed = arrived.map(|e| 0.09 * (1. - ease_out(progress(e, 1600))));
        let flash = picked.or(landed);
        if flash.is_some() {
            window.request_animation_frame();
        }
        div()
            .id(("history-row", id as usize))
            .when_some(flash, |d, f| d.bg(p.primary.opacity(f)))
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                if *hovered {
                    this.hovered_row = Some(id);
                } else if this.hovered_row == Some(id) {
                    this.hovered_row = None;
                }
                cx.notify();
            }))
            .flex()
            .gap(px(12.))
            .rounded(px(12.))
            .px(px(12.))
            .py(px(10.))
            .hover(move |s| {
                s.bg(if p.dark {
                    p.white.opacity(0.03)
                } else {
                    p.muted.opacity(0.3)
                })
            })
            .when(route_translation, |d| {
                d.border_l_2().border_color(p.primary.opacity(0.7))
            })
            .child(
                div()
                    .w(px(56.))
                    .flex_none()
                    .pt(px(4.))
                    .text_size(px(TEXT_CAPTION))
                    .text_color(p.muted_foreground.opacity(0.65))
                    .child(format_time(item)),
            )
            .child(div().flex_1().min_w_0().child(body).children(raw_section))
            .child(actions)
            .into_any_element()
    }

    /// The row a Dictation lands in: a shimmering label over two skeleton
    /// lines, opening over 300 ms.
    fn render_pending(
        &self,
        since: Instant,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = cx.palette();
        let time = match i18n::language() {
            Language::ZhCn => Local::now().format("%H:%M").to_string(),
            Language::En => Local::now().format("%I:%M %p").to_string(),
        };
        div()
            .flex()
            .gap(px(12.))
            .rounded(px(12.))
            .px(px(12.))
            .py(px(10.))
            .overflow_hidden()
            .child(
                div()
                    .w(px(56.))
                    .flex_none()
                    .pt(px(4.))
                    .text_size(px(TEXT_CAPTION))
                    .text_color(p.muted_foreground.opacity(0.65))
                    .child(time),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .pt(px(2.))
                    .child(
                        div()
                            .text_size(px(TEXT_BODY))
                            .text_color(p.muted_foreground)
                            .child(shimmer_text(
                                t("controlPanel.history.transcribing"),
                                since,
                                &p,
                                window,
                                cx,
                            )),
                    )
                    .child(
                        div()
                            .mt(px(8.))
                            .flex()
                            .flex_col()
                            .gap(px(7.))
                            .child(skeleton_line(0.94, 0, since, &p, cx))
                            .child(skeleton_line(0.71, 1, since, &p, cx)),
                    ),
            )
            .with_animation(
                "pending-in",
                Animation::new(Duration::from_millis(DUR_SHEET)).with_easing(ease_snap),
                |d, t| d.opacity(t).max_h(px(96. * t)),
            )
            .into_any_element()
    }

    /// Tracks the transcribing placeholder. Once the new transcript tops the
    /// list it takes over.
    fn sync_pending(&mut self, busy: bool, top: Option<i64>) {
        // Checked before `busy`: without auto-paste the row lands in the same
        // update that ends the Dictation.
        if let Some(pending) = self.pending.as_mut()
            && !pending.arrived
            && top != pending.base_top
        {
            pending.arrived = true;
            if let Some(id) = top {
                self.arrived = Some((id, Instant::now()));
            }
        }
        if !busy {
            self.pending = None;
        } else if self.pending.is_none() {
            self.pending = Some(PendingRow {
                base_top: top,
                since: Instant::now(),
                arrived: false,
            });
        }
    }

    fn render_menu(&self, item: &TranscriptionItem, cx: &mut Context<Self>) -> AnyElement {
        let p = cx.palette();
        let id = item.id;
        let mut entries: Vec<(&'static str, String, u8, bool)> = Vec::new();
        if item.has_audio {
            let label = if item.route_kind.as_deref() == Some("translation") {
                t("controlPanel.history.retryTranslationMode")
            } else {
                t("controlPanel.history.retryTranscription")
            };
            entries.push(("rotate-ccw", label, 0, false));
        }
        if item.raw_text.is_some() {
            entries.push((
                "file-text",
                t("controlPanel.history.viewRawTranscript"),
                1,
                false,
            ));
        }
        if item.has_audio {
            entries.push((
                "folder-open",
                t("controlPanel.history.showInFolder"),
                2,
                false,
            ));
        }
        entries.push(("trash2", t("controlPanel.history.deleteItem"), 3, true));

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
                            0 => this.retry(id, cx),
                            1 => {
                                if !this.expanded.remove(&id) {
                                    this.expanded.insert(id);
                                }
                            }
                            2 => Self::show_in_folder(id, cx),
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
        let (history, loaded, retention, hotkey) = {
            let s = state.read(cx);
            (
                s.history.clone(),
                s.history_loaded,
                s.settings.data_retention_enabled,
                s.settings.effective_dictation_key(),
            )
        };
        let pending = self
            .pending
            .as_ref()
            .filter(|p| !p.arrived)
            .map(|p| p.since);

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
        } else if history.is_empty() && pending.is_none() {
            content = content.child(render_empty_with_hotkey(&hotkey, &p));
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
            // The placeholder belongs to today, which may not have a group yet.
            let today = format_date_group(Local::now().date_naive());
            if pending.is_some() && groups.first().is_none_or(|(label, _)| *label != today) {
                groups.insert(0, (today, Vec::new()));
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
                if index == 0
                    && let Some(since) = pending
                {
                    rows = rows.child(self.render_pending(since, window, cx));
                }
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

        div().px(px(20.)).pt(px(20.)).pb(px(32.)).child(
            div()
                .mx_auto()
                .max_w(px(READING_MAX_WIDTH))
                .flex()
                .child(content),
        )
    }
}

fn render_empty_with_hotkey(hotkey: &str, p: &Palette) -> AnyElement {
    let mut keys = div().flex().items_center().gap(px(4.));
    for (i, key) in backend::parse_hotkey_list(hotkey).into_iter().enumerate() {
        if i > 0 {
            keys = keys.child(div().text_color(p.foreground.opacity(0.45)).child("/"));
        }
        keys = keys.child(kbd(backend::format_hotkey_label(&key), p));
    }
    empty_card(keys, p)
}

fn empty_card(keys: Div, p: &Palette) -> AnyElement {
    div()
        .child(
            div()
                .pt(px(8.))
                .pb(px(10.))
                .text_size(px(TEXT_BODY))
                .text_color(p.muted_foreground)
                .child(t("controlPanel.history.sectionTitle")),
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
