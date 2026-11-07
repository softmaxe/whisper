//! Home: the local History grouped by day.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use chrono::{DateTime, Local, NaiveDate, Utc};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::db::TranscriptionItem;
use crate::i18n::{self, Language, t, t_with};

use crate::app_state::AppState;
use crate::backend;

use super::components::{
    KeyboardFocusable, MenuTriggerBounds, empty_state, f32_tx, icon, kbd, keyed_focus_handle,
    page_container, retention_banner, shimmer_text, skeleton_line, spinner, text_tooltip,
};
use super::overlays::{DialogRequest, Toast, ToastVariant, show_dialog, show_toast};
use super::theme::{
    DUR_BASE, DUR_FAST, DUR_SHEET, DUR_SNAP, FONT_VOICE, LINE_READING, LINE_SMALL, Palette,
    PaletteExt, RADIUS_LG, RADIUS_MD, READING_MAX_WIDTH, TEXT_BODY, TEXT_CAPTION, TEXT_READING,
    TEXT_SMALL, ease_out, ease_snap, motion, progress, tabular_nums,
};

/// Width of the clock column; fits "12:59 PM" so the text column never shifts.
const TIME_COLUMN: f32 = 64.;
/// The list stops where its reading column and row actions end, so the
/// actions sit beside the text they act on rather than at the window edge.
const LIST_MAX_WIDTH: f32 = TIME_COLUMN + READING_MAX_WIDTH + 144.;

/// How long a new row stays tinted.
const FLASH: Duration = Duration::from_millis(1600);
/// How long the rest of the list stays dimmed around a row picked in the
/// search palette, fades included.
const SPOTLIGHT: Duration = Duration::from_millis(2300);
/// How much of each other row's opacity the spotlight takes away.
const SPOTLIGHT_DIM: f32 = 0.72;

/// The placeholder row shown while a Dictation is transcribing.
struct PendingRow {
    /// The top row when transcription started; a new top row replaces the placeholder.
    base_top: Option<i64>,
    since: Instant,
    arrived: bool,
}

/// A row picked in the search palette, held in focus until the spotlight ends.
struct Spotlight {
    id: i64,
    since: Instant,
    /// The row's bounds are unknown until it is laid out, so centering waits
    /// for its prepaint.
    center_pending: bool,
}

/// The panel's eased scroll to a picked row.
struct Glide {
    from: Pixels,
    to: Pixels,
    since: Instant,
}

pub struct HistoryView {
    /// The control panel's scroll area, which holds this view.
    scroll: ScrollHandle,
    spotlight: Option<Spotlight>,
    glide: Option<Glide>,
    expanded: HashSet<i64>,
    retrying: HashSet<i64>,
    open_menu: Option<i64>,
    hovered_row: Option<i64>,
    pending: Option<PendingRow>,
    /// A row that just landed from Dictation; it reveals and tints briefly.
    arrived: Option<(i64, Instant)>,
    /// A transcript removed from view whose deletion waits out the Undo
    /// window; the database keeps it until then.
    deleting: Option<i64>,
    /// The row the arrow keys point at while the list has keyboard focus.
    cursor: Option<i64>,
    /// A row to scroll into view once it is laid out.
    reveal_pending: Option<i64>,
}

/// How long a deleted transcript can be brought back.
const UNDO_WINDOW: Duration = Duration::from_millis(6000);

pub fn copy_with_toast(text: String, cx: &mut App) {
    cx.write_to_clipboard(ClipboardItem::new_string(text));
    show_toast(
        Toast::new(t("controlPanel.history.copiedTitle"))
            .description(t("controlPanel.history.copiedDescription"))
            .variant(ToastVariant::Success)
            .duration(Duration::from_millis(4000)),
        cx,
    );
}

fn local_time(item: &TranscriptionItem) -> Option<DateTime<Local>> {
    item.occurred_at()
        .map(|t: DateTime<Utc>| t.with_timezone(&Local))
}

fn format_time(item: &TranscriptionItem) -> String {
    local_time(item).map(format_clock).unwrap_or_default()
}

/// A clock time in the UI language's convention: 24-hour for Chinese,
/// 12-hour with AM/PM for English.
fn format_clock(time: DateTime<Local>) -> String {
    match i18n::language() {
        Language::ZhCn => time.format("%H:%M").to_string(),
        Language::En => time.format("%-I:%M %p").to_string(),
    }
}

pub fn format_date_group(date: NaiveDate) -> String {
    super::format::day_label(date, Local::now().date_naive())
}

impl HistoryView {
    pub fn new(scroll: ScrollHandle, cx: &mut Context<Self>) -> Self {
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
        if !state.read(cx).history_loaded {
            Self::reload(cx);
        }
        Self {
            scroll,
            spotlight: None,
            glide: None,
            expanded: HashSet::new(),
            retrying: HashSet::new(),
            open_menu: None,
            hovered_row: None,
            pending: None,
            arrived: None,
            deleting: None,
            cursor: None,
            reveal_pending: None,
        }
    }

    pub fn highlight(&mut self, id: i64, cx: &mut Context<Self>) {
        self.spotlight = Some(Spotlight {
            id,
            since: Instant::now(),
            center_pending: true,
        });
        cx.notify();
    }

    /// Starts the scroll that centers a picked row in the panel. Runs during
    /// prepaint, where the panel's scroll bounds are current.
    fn center_row(&mut self, row: Bounds<Pixels>, reduce_motion: bool) {
        let Some(spotlight) = self.spotlight.as_mut() else {
            return;
        };
        spotlight.center_pending = false;
        let offset = self.scroll.offset();
        let shift = self.scroll.bounds().center().y - row.center().y;
        let to = (offset.y + shift).clamp(-self.scroll.max_offset().y, px(0.));
        if reduce_motion {
            self.scroll.set_offset(point(offset.x, to));
        } else {
            self.glide = Some(Glide {
                from: offset.y,
                to,
                since: Instant::now(),
            });
        }
    }

    fn advance_glide(&mut self) {
        let Some(glide) = &self.glide else {
            return;
        };
        let t = ease_out(progress(glide.since.elapsed(), DUR_SHEET));
        let y = glide.from + (glide.to - glide.from) * t;
        self.scroll.set_offset(point(self.scroll.offset().x, y));
        if t >= 1. {
            self.glide = None;
        }
    }

    /// A click ends the spotlight early, through its normal fade.
    fn release_spotlight(&mut self, cx: &mut Context<Self>) {
        let fade = Duration::from_millis(DUR_BASE);
        if let Some(spotlight) = self.spotlight.as_mut()
            && spotlight.since.elapsed() < SPOTLIGHT - fade
        {
            spotlight.since = Instant::now() - (SPOTLIGHT - fade);
            cx.notify();
        }
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

    /// Removes a transcript from view at once and deletes it when the Undo
    /// window closes, the way Dictionary removes a word. Clear History,
    /// which cannot be undone, still asks first.
    fn delete(&mut self, id: i64, cx: &mut Context<Self>) {
        self.commit_delete(cx);
        self.deleting = Some(id);
        if self.cursor == Some(id) {
            self.cursor = None;
        }
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(UNDO_WINDOW).await;
            let _ = this.update(cx, |this, cx| {
                if this.deleting == Some(id) {
                    this.commit_delete(cx);
                }
            });
        })
        .detach();
        let this = cx.weak_entity();
        show_toast(
            Toast::new(t("controlPanel.history.deletedTitle"))
                .duration(UNDO_WINDOW)
                .action(t("common.undo"), move |_, cx| {
                    let _ = this.update(cx, |this, cx| {
                        if this.deleting == Some(id) {
                            this.deleting = None;
                            cx.notify();
                        }
                    });
                }),
            cx,
        );
        cx.notify();
    }

    /// Deletes the transcript waiting out its Undo window, if any.
    fn commit_delete(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.deleting.take() else {
            return;
        };
        let result = AppState::global(cx).update(cx, |s, cx| s.delete_transcription(id, cx));
        let description = match result {
            Ok(true) => return,
            Ok(false) => t("controlPanel.history.couldNotDeleteDescription"),
            Err(_) => t("controlPanel.history.couldNotDeleteDescriptionGeneric"),
        };
        show_dialog(
            DialogRequest::alert(
                t("controlPanel.history.couldNotDeleteTitle"),
                Some(description.into()),
            ),
            cx,
        );
        cx.notify();
    }

    /// Arrow keys move a row cursor through the list; Return copies the row,
    /// Space shows its raw transcript, and ⌘⌫ deletes it.
    fn on_list_key(&mut self, e: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let ids: Vec<i64> = AppState::global(cx)
            .read(cx)
            .history
            .iter()
            .map(|item| item.id)
            .filter(|id| Some(*id) != self.deleting)
            .collect();
        if ids.is_empty() {
            return;
        }
        let at = self.cursor.and_then(|c| ids.iter().position(|id| *id == c));
        let key = e.keystroke.key.as_str();
        let cmd = e.keystroke.modifiers.platform;
        match key {
            "down" | "up" if !keyed_focus_handle("history-list".into(), cx).is_focused(window) => {
                // Arrows from a row's button hand focus back to the list.
                let list = keyed_focus_handle("history-list".into(), cx);
                window.focus(&list, cx);
                self.cursor = self.cursor.or(ids.first().copied());
                self.reveal_pending = self.cursor;
            }
            "down" => {
                let next = at.map_or(0, |i| (i + 1).min(ids.len() - 1));
                self.cursor = Some(ids[next]);
                self.reveal_pending = self.cursor;
            }
            "up" => {
                let prev = at.map_or(ids.len() - 1, |i| i.saturating_sub(1));
                self.cursor = Some(ids[prev]);
                self.reveal_pending = self.cursor;
            }
            "enter" => {
                let Some(id) = self.cursor else { return };
                let text = AppState::global(cx)
                    .read(cx)
                    .history
                    .iter()
                    .find(|item| item.id == id)
                    .map(|item| item.text.clone());
                if let Some(text) = text {
                    copy_with_toast(text, cx);
                }
            }
            "space" => {
                let Some(id) = self.cursor else { return };
                if !self.expanded.remove(&id) {
                    self.expanded.insert(id);
                }
            }
            "backspace" | "delete" if cmd => {
                let Some(id) = self.cursor else { return };
                // Keep the cursor in place on the next row.
                let next = at.and_then(|i| {
                    ids.get(i + 1)
                        .or_else(|| i.checked_sub(1).and_then(|p| ids.get(p)))
                });
                let next = next.copied();
                self.delete(id, cx);
                self.cursor = next;
            }
            "escape" if self.cursor.is_some() => self.cursor = None,
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }

    /// Scrolls just enough to show a row the keyboard moved to.
    fn reveal_row(&mut self, row: Bounds<Pixels>) {
        self.reveal_pending = None;
        let view = self.scroll.bounds();
        // Room for the top bar's fade above and a little air below.
        let top = view.top() + px(40.);
        let bottom = view.bottom() - px(16.);
        let offset = self.scroll.offset();
        let shift = if row.top() < top {
            top - row.top()
        } else if row.bottom() > bottom {
            bottom - row.bottom()
        } else {
            return;
        };
        let to = (offset.y + shift).clamp(-self.scroll.max_offset().y, px(0.));
        self.scroll.set_offset(point(offset.x, to));
    }

    /// Asks before removing every transcript, its audio and the Insights
    /// counters. Reached from the top bar while Home shows.
    pub fn clear_all(cx: &mut App) {
        let mut request = DialogRequest::confirm(
            t("controlPanel.history.clearAllTitle"),
            Some(t("controlPanel.history.clearAllDescriptionDevice").into()),
            true,
            |_, cx| {
                let result = AppState::global(cx).update(cx, |s, cx| s.clear_transcriptions(cx));
                if result.is_ok() {
                    show_toast(
                        Toast::new(t("controlPanel.history.clearAllSuccess"))
                            .variant(ToastVariant::Success)
                            .duration(Duration::from_millis(4000)),
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
        request.confirm_label = t("controlPanel.history.clearHistoryConfirm").into();
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
        // A raw transcript identical to the text has nothing to reveal.
        let raw_text = item.raw_text.clone().filter(|raw| *raw != item.text);
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
            .max_w(px(READING_MAX_WIDTH))
            .font_family(FONT_VOICE)
            .text_size(px(TEXT_READING))
            .line_height(px(LINE_READING))
            .text_color(p.foreground)
            .child(item.text.clone())
            .into_any_element();

        // The toggle stays in place so the same click opens and closes the panel.
        // The raw transcript opens from the row's actions; once open, its
        // header here closes it again and copies it.
        let has_raw = raw_text.is_some();
        let raw_toggle = raw_text
            .as_ref()
            .filter(|_| raw_progress > 0.001)
            .map(|raw| {
                let copy_raw = raw.clone();
                div()
                    .mt(px(6.))
                    .opacity(raw_progress)
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .id(("raw-toggle", id as usize))
                            .keyboard_focusable(
                                ("history-raw", id as usize),
                                Role::Button,
                                t("controlPanel.history.rawTranscript"),
                                cx,
                            )
                            .aria_expanded(expanded)
                            .h(px(24.))
                            .ml(px(-6.))
                            .px(px(6.))
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .rounded(px(RADIUS_MD))
                            .text_size(px(TEXT_SMALL))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(p.muted_foreground)
                            .cursor_pointer()
                            .hover(|s| s.bg(p.hover_wash(0.06)).text_color(p.foreground))
                            .child(t("controlPanel.history.rawTranscript"))
                            .child(
                                icon("chevron-down", 10., p.muted_foreground).with_transformation(
                                    Transformation::rotate(radians(
                                        raw_progress * std::f32::consts::PI,
                                    )),
                                ),
                            )
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if !this.expanded.remove(&id) {
                                    this.expanded.insert(id);
                                }
                                cx.notify();
                            })),
                    )
                    .when(raw_progress > 0.001, |d| {
                        d.child(
                            div()
                                .id(("copy-raw", id as usize))
                                .keyboard_focusable(
                                    ("history-copy-raw", id as usize),
                                    Role::Button,
                                    t("controlPanel.history.copyRaw"),
                                    cx,
                                )
                                .tooltip(text_tooltip(t("controlPanel.history.copyRaw")))
                                .size(px(24.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded(px(RADIUS_MD))
                                .opacity(raw_progress)
                                .cursor_pointer()
                                .hover(|s| s.bg(p.hover_wash(0.06)))
                                .child(icon("copy", 12., p.muted_foreground))
                                .on_click(move |_, _, cx| copy_with_toast(copy_raw.clone(), cx)),
                        )
                    })
            });

        let raw_section = raw_text.filter(|_| raw_progress > 0.001).map(|raw| {
            div()
                .overflow_hidden()
                .opacity(raw_progress)
                // The cap only animates the height; a settled panel fits its text.
                .when(raw_progress < 0.999, |d| d.max_h(px(400. * raw_progress)))
                .child(
                    div()
                        .relative()
                        .top(px(3. * (1. - raw_progress)))
                        .font_family(FONT_VOICE)
                        .text_size(px(TEXT_BODY))
                        .line_height(px(20.))
                        .text_color(p.subtle_foreground)
                        .child(raw),
                )
        });

        let menu_trigger = MenuTriggerBounds::default();
        let action_btn = |id_str: &'static str, name: &'static str, label: String, cx: &mut App| {
            div()
                .id((id_str, id as usize))
                .keyboard_focusable((id_str, id as usize), Role::Button, label.clone(), cx)
                .tooltip(text_tooltip(label))
                .size(px(28.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(RADIUS_LG))
                .cursor_pointer()
                .hover(move |s| s.bg(p.hover_wash(0.06)))
                .child(icon(name, 14., p.muted_foreground))
        };

        // Row actions surface on hover, or while one of them holds keyboard
        // focus, in 120 ms and leave in 60 ms.
        let focus_within = ["raw-row", "copy-row", "more-row"]
            .iter()
            .any(|key| keyed_focus_handle((*key, id as usize).into(), cx).is_focused(window));
        let cursor_here = self.cursor == Some(id)
            && keyed_focus_handle("history-list".into(), cx).contains_focused(window, cx);
        let actions_shown =
            menu_open || self.hovered_row == Some(id) || focus_within || cursor_here;
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
            .when(has_raw, |d| {
                let label = t(if expanded {
                    "controlPanel.history.hideRaw"
                } else {
                    "controlPanel.history.showRaw"
                });
                d.child(
                    action_btn("raw-row", "eye", label, cx)
                        .aria_expanded(expanded)
                        .when(expanded, |d| d.bg(p.hover_wash(0.08)))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if !this.expanded.remove(&id) {
                                this.expanded.insert(id);
                            }
                            cx.notify();
                        })),
                )
            })
            .child(
                action_btn("copy-row", "copy", t("controlPanel.history.copyText"), cx)
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
                        action_btn(
                            "more-row",
                            "more-vertical",
                            t("controlPanel.history.moreActions"),
                            cx,
                        )
                        .relative()
                        .child(menu_trigger.measure())
                        .aria_expanded(menu_open)
                        .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                            this.toggle_menu(id, event.is_keyboard(), window, cx)
                        }))
                        .into_any_element()
                    })
                    .when(menu_open, |d| {
                        d.child(self.render_menu(item, menu_trigger, cx))
                    }),
            );

        // A new transcript tints in and eases out over the flash.
        let landed = arrived.map(|e| 0.09 * (1. - ease_out(progress(e, FLASH.as_millis() as u64))));
        if landed.is_some() {
            window.request_animation_frame();
        }
        let reduce_motion = cx.reduce_motion();
        let spotlight = self.spotlight.as_ref().map(|s| {
            (
                s.id == id,
                spotlight_level(s.since.elapsed(), reduce_motion),
            )
        });
        let center = self
            .spotlight
            .as_ref()
            .is_some_and(|s| s.id == id && s.center_pending);
        let this = cx.weak_entity();
        let reveal = this.clone();
        let row_wash = if p.dark {
            p.white.opacity(0.04)
        } else {
            p.foreground.opacity(0.045)
        };
        div()
            .id(("history-row", id as usize))
            .role(Role::ListItem)
            .aria_label(item.text.clone())
            .when(cursor_here, |d| d.aria_active_descendant().bg(row_wash))
            .when(self.reveal_pending == Some(id), |d| {
                d.child(
                    canvas(
                        move |bounds, _, cx| {
                            let _ = reveal.update(cx, |this, _| this.reveal_row(bounds));
                        },
                        |_, _, _, _| {},
                    )
                    .absolute()
                    .size_full(),
                )
            })
            .relative()
            .when_some(landed, |d, f| d.bg(p.primary.opacity(f)))
            // The picked row keeps the palette's selection tint; the rest recede.
            .when_some(spotlight, |d, (picked, level)| {
                if picked {
                    d.bg(p.foreground.opacity(0.09 * level))
                } else {
                    d.opacity(1. - SPOTLIGHT_DIM * level)
                }
            })
            .when(center, |d| {
                d.child(
                    canvas(
                        move |bounds, _, cx| {
                            let _ = this.update(cx, |this, _| {
                                this.center_row(bounds, reduce_motion);
                            });
                        },
                        |_, _, _, _| {},
                    )
                    .absolute()
                    .size_full(),
                )
            })
            // Right-click opens the same menu as the row's more button.
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, _, window, cx| {
                    if this.open_menu != Some(id) {
                        this.toggle_menu(id, false, window, cx);
                    }
                }),
            )
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
            .rounded(px(RADIUS_LG))
            .px(px(12.))
            .py(px(10.))
            .hover(move |s| {
                s.bg(if p.dark {
                    p.white.opacity(0.04)
                } else {
                    p.foreground.opacity(0.045)
                })
            })
            .child(
                div()
                    .w(px(TIME_COLUMN))
                    .flex_none()
                    .pt(px(4.))
                    .text_right()
                    .text_size(px(TEXT_SMALL))
                    .font_features(tabular_nums())
                    .text_color(p.subtle_foreground)
                    .child(format_time(item)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(body)
                    .children(raw_toggle)
                    .children(raw_section),
            )
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
        let time = format_clock(Local::now());
        div()
            .flex()
            .gap(px(12.))
            .rounded(px(RADIUS_LG))
            .px(px(12.))
            .py(px(10.))
            .overflow_hidden()
            .child(
                div()
                    .w(px(TIME_COLUMN))
                    .flex_none()
                    .pt(px(4.))
                    .text_right()
                    .text_size(px(TEXT_SMALL))
                    .font_features(tabular_nums())
                    .text_color(p.subtle_foreground)
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
                motion(DUR_SHEET, cx).with_easing(ease_snap),
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

    /// Opens or closes a row's menu. Opened from the keyboard, it focuses
    /// its first item so arrows, Return and Escape work at once.
    fn toggle_menu(
        &mut self,
        id: i64,
        keyboard: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_menu = if self.open_menu == Some(id) {
            None
        } else {
            Some(id)
        };
        if keyboard && self.open_menu.is_some() {
            // Retry and Reveal need audio; Delete is always there.
            let has_audio = AppState::global(cx)
                .read(cx)
                .history
                .iter()
                .any(|item| item.id == id && item.has_audio);
            let first_action: usize = if has_audio { 0 } else { 2 };
            let first = keyed_focus_handle(("history-menu-item", first_action).into(), cx);
            window.on_next_frame(move |window, cx| window.focus(&first, cx));
        }
        cx.notify();
    }

    fn close_menu(&mut self, id: i64, window: &mut Window, cx: &mut Context<Self>) {
        self.open_menu = None;
        // Hand focus back to the button that opened the menu.
        let in_menu = (0..3usize).any(|action| {
            keyed_focus_handle(("history-menu-item", action).into(), cx).is_focused(window)
        });
        if in_menu {
            let more = keyed_focus_handle(("more-row", id as usize).into(), cx);
            window.focus(&more, cx);
        }
        cx.notify();
    }

    fn render_menu(
        &self,
        item: &TranscriptionItem,
        trigger: MenuTriggerBounds,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = cx.palette();
        let id = item.id;
        let mut entries: Vec<(&'static str, String, u8, bool)> = Vec::new();
        if item.has_audio {
            entries.push((
                "rotate-ccw",
                t("controlPanel.history.retryTranscription"),
                0,
                false,
            ));
        }
        if item.has_audio {
            entries.push((
                "folder-open",
                t("controlPanel.history.showInFolder"),
                1,
                false,
            ));
        }
        entries.push(("trash2", t("controlPanel.history.deleteItem"), 2, true));

        let mut menu = div()
            .id(("row-menu", id as usize))
            .occlude()
            .mt(px(4.))
            .min_w(px(180.))
            .p(px(4.))
            .flex()
            .flex_col()
            .rounded(px(RADIUS_LG))
            .border_1()
            .border_color(if p.dark {
                p.border_hover
            } else {
                p.border.opacity(0.7)
            })
            .bg(if p.dark { p.surface_3 } else { p.popover })
            .shadow_lg()
            .on_mouse_down_out(cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                if !trigger.contains(event.position) {
                    this.open_menu = None;
                    cx.notify();
                }
            }))
            .on_key_down(cx.listener(move |this, e: &KeyDownEvent, window, cx| {
                match e.keystroke.key.as_str() {
                    "escape" => this.close_menu(id, window, cx),
                    // Tab leaves the menu: close it so it never floats over
                    // the page while focus is elsewhere.
                    "tab" => {
                        this.close_menu(id, window, cx);
                        return;
                    }
                    "down" => window.focus_next(cx),
                    "up" => window.focus_prev(cx),
                    _ => return,
                }
                cx.stop_propagation();
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
                    .keyboard_focusable(
                        ("history-menu-item", action as usize),
                        Role::MenuItem,
                        label.clone(),
                        cx,
                    )
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .px(px(8.))
                    .py(px(6.))
                    .rounded(px(RADIUS_MD))
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
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.close_menu(id, window, cx);
                        match action {
                            0 => this.retry(id, cx),
                            1 => Self::show_in_folder(id, cx),
                            _ => this.delete(id, cx),
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
        self.spotlight.take_if(|s| s.since.elapsed() >= SPOTLIGHT);
        self.advance_glide();
        if self.spotlight.is_some() || self.glide.is_some() {
            window.request_animation_frame();
        }
        let spotlit = self.spotlight.is_some();
        let state = AppState::global(cx);
        let (history, retention, hotkey) = {
            let s = state.read(cx);
            (
                s.history.clone(),
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
                retention_banner(
                    t("controlPanel.history.dataRetentionDisabled"),
                    Some(LINE_SMALL),
                    &p,
                )
                .mb(px(12.)),
            );
        }

        // A transcript waiting out its Undo window is already gone here.
        let history: Vec<TranscriptionItem> = history
            .into_iter()
            .filter(|item| Some(item.id) != self.deleting)
            .collect();
        if history.is_empty() && pending.is_none() {
            content = content.child(render_empty_with_hotkey(&hotkey, retention, &p));
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
            let list_focus = keyed_focus_handle("history-list".into(), cx);
            let mut list = div()
                .id("history-list")
                .track_focus(&list_focus.tab_stop(true))
                .role(Role::List)
                .aria_label(t("sidebar.home"))
                .on_key_down(cx.listener(Self::on_list_key));
            for (index, (label, items)) in groups.into_iter().enumerate() {
                // Day label with its count beside it, then a hairline to the edge.
                let count = items.len();
                let header = div()
                    .id(SharedString::from(format!("history-day-{index}")))
                    .role(Role::Heading)
                    .aria_label(t_with(
                        "controlPanel.history.dayCount",
                        &[("day", &label), ("count", &count.to_string())],
                    ))
                    .relative()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .h(px(32.))
                    .mb(px(4.))
                    .bg(p.background)
                    .text_size(px(TEXT_SMALL))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(p.muted_foreground)
                    .child(div().flex_none().child(label.clone()))
                    // The count is a quiet badge, so "Nov 3 6" never reads as
                    // part of the date.
                    .child(
                        div()
                            .flex_none()
                            .px(px(6.))
                            .rounded_full()
                            .bg(p.hover_wash(if p.dark { 0.08 } else { 0.06 }))
                            .text_size(px(TEXT_CAPTION))
                            .line_height(px(16.))
                            .font_weight(FontWeight::MEDIUM)
                            .font_features(tabular_nums())
                            .text_color(p.muted_foreground)
                            .child(count.to_string()),
                    )
                    .child(
                        div()
                            .ml(px(4.))
                            .h(px(1.))
                            .flex_1()
                            .bg(p.border.opacity(0.5)),
                    );
                // Rows stop where the reading column and its actions end,
                // so actions sit beside the text; day headers span the page.
                let mut rows = div()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .mx(px(-12.))
                    .max_w(px(LIST_MAX_WIDTH + 24.));
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
                        .when(index > 0, |d| d.mt(px(28.)))
                        .child(header)
                        .child(rows),
                );
            }
            content = content.child(list);
        }

        page_container()
            .pb(px(48.))
            .when(spotlit, |d| {
                d.on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, _, cx| this.release_spotlight(cx)),
                )
            })
            .child(div().flex().child(content))
    }
}

/// How strong the spotlight is after `elapsed`: it fades in, holds, and fades
/// out before [`SPOTLIGHT`] ends. Reduced motion holds it without the fades.
fn spotlight_level(elapsed: Duration, reduce_motion: bool) -> f32 {
    if reduce_motion {
        return 1.;
    }
    let fade = Duration::from_millis(DUR_BASE);
    let fade_in = ease_out(progress(elapsed, DUR_BASE));
    let fade_out = ease_out(progress(elapsed.saturating_sub(SPOTLIGHT - fade), DUR_BASE));
    fade_in * (1. - fade_out)
}

fn render_empty_with_hotkey(hotkey: &str, retention: bool, p: &Palette) -> AnyElement {
    let mut keys = div().flex().items_center().gap(px(4.));
    for (i, key) in crate::text::hotkeys::parse_hotkey_list(hotkey)
        .iter()
        .enumerate()
    {
        if i > 0 {
            keys = keys.child(div().text_color(p.foreground.opacity(0.45)).child("/"));
        }
        keys = keys.child(kbd(
            crate::text::hotkeys::format_hotkey_list_label(&key.to_string()),
            p,
        ));
    }
    empty_card(keys, retention, p)
}

/// With History off, the empty state says why nothing will appear rather
/// than promising that transcripts will.
fn empty_card(keys: Div, retention: bool, p: &Palette) -> AnyElement {
    let body = t(if retention {
        "controlPanel.history.emptyDescription"
    } else {
        "controlPanel.history.emptyDescriptionOff"
    });
    div()
        .flex()
        .flex_col()
        .items_center()
        .min_h(px(256.))
        .px(px(40.))
        .py(px(48.))
        .child(empty_state("mic", t("controlPanel.history.empty"), body, p))
        .child(
            div()
                .mt(px(20.))
                .h(px(32.))
                .flex()
                .items_center()
                .gap(px(6.))
                .px(px(12.))
                .rounded(px(RADIUS_LG))
                .border_1()
                .border_color(p.border)
                .text_size(px(TEXT_SMALL))
                .font_weight(FontWeight::MEDIUM)
                .text_color(p.muted_foreground)
                .child(t("controlPanel.history.press"))
                .child(keys)
                .child(t("controlPanel.history.toStart")),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::{Duration, SPOTLIGHT, spotlight_level};

    #[test]
    fn spotlight_fades_in_holds_and_fades_out() {
        assert_eq!(spotlight_level(Duration::ZERO, false), 0.);
        assert_eq!(spotlight_level(SPOTLIGHT / 2, false), 1.);
        assert_eq!(spotlight_level(SPOTLIGHT, false), 0.);
    }

    #[test]
    fn spotlight_skips_the_fades_with_reduced_motion() {
        assert_eq!(spotlight_level(Duration::ZERO, true), 1.);
    }
}
