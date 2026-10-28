//! The main window: collapsible sidebar, top bar with the transcript search,
//! and the active view inside an inset rounded container.

use std::time::Duration;

use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::i18n::t;

use crate::app_state::AppState;

use super::command_search::{CommandSearch, CommandSearchEvent};
use super::components::{f32_tx, icon};
use super::dictionary_view::DictionaryView;
use super::history_view::HistoryView;
use super::insights_view::InsightsView;
use super::overlays::{Overlays, render_dialog, render_toasts};
use super::settings_modal::{SettingsModal, SettingsModalEvent, SettingsSection};
use super::theme::{
    DUR_BASE, DUR_FAST, DUR_SHEET, DUR_SNAP, FONT_MONO, FONT_UI, LINE_TITLE, PaletteExt, TEXT_BODY,
    TEXT_CAPTION, TEXT_TITLE, ease_out, ease_snap,
};
use super::upload_view::{UploadEvent, UploadView};

actions!(control_panel, [OpenSearch, OpenSettings, CloseOverlay]);

pub const SIDEBAR_WIDTH: f32 = 192.;
const PEEK_CLOSE_DELAY: Duration = Duration::from_millis(DUR_SNAP);
/// Height of the fade that softens content scrolled under the top bar.
const SCROLL_FADE: f32 = 28.;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanelView {
    Home,
    Insights,
    Upload,
    Dictionary,
}

impl PanelView {
    pub const ALL: [PanelView; 4] = [Self::Home, Self::Insights, Self::Upload, Self::Dictionary];

    pub fn label(self) -> String {
        t(match self {
            Self::Home => "sidebar.home",
            Self::Insights => "sidebar.insights",
            Self::Upload => "sidebar.upload",
            Self::Dictionary => "sidebar.dictionary",
        })
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::Home => "home",
            Self::Insights => "bar-chart3",
            Self::Upload => "upload",
            Self::Dictionary => "book-open",
        }
    }
}

pub struct ControlPanel {
    pub active: PanelView,
    sidebar_collapsed: bool,
    peek: bool,
    /// After a toggle collapses the sidebar, the pointer still sits on the
    /// toggle; suppress peeking until it leaves so the sidebar doesn't bounce.
    suppress_peek: bool,
    peek_close: Option<Task<()>>,
    focus: FocusHandle,
    history: Entity<HistoryView>,
    insights: Option<Entity<InsightsView>>,
    dictionary: Option<Entity<DictionaryView>>,
    upload: Entity<UploadView>,
    settings: Option<Entity<SettingsModal>>,
    scroll: ScrollHandle,
}

impl ControlPanel {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let collapsed = AppState::settings(cx).sidebar_collapsed;
        let state = AppState::global(cx);
        cx.observe(&state, |_, _, cx| cx.notify()).detach();
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let overlays = Overlays::global(cx);
        cx.observe(&overlays, |_, _, cx| cx.notify()).detach();
        let history = cx.new(HistoryView::new);
        let upload = cx.new(UploadView::new);
        cx.subscribe_in(
            &upload,
            window,
            |this, _, event: &UploadEvent, window, cx| match event {
                UploadEvent::OpenHistory => this.set_view(PanelView::Home, cx),
                UploadEvent::OpenSettings(section) => {
                    this.open_settings(Some(SettingsSection::from_name(section)), window, cx)
                }
            },
        )
        .detach();
        Self {
            active: PanelView::Home,
            sidebar_collapsed: collapsed,
            peek: false,
            suppress_peek: false,
            peek_close: None,
            focus,
            history,
            insights: None,
            dictionary: None,
            upload,
            settings: None,
            scroll: ScrollHandle::new(),
        }
    }

    pub fn open_settings(
        &mut self,
        section: Option<SettingsSection>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(modal) = &self.settings {
            if let Some(section) = section {
                modal.update(cx, |m, cx| m.set_section(section, cx));
            }
            return;
        }
        let modal = cx
            .new(|cx| SettingsModal::new(section.unwrap_or(SettingsSection::General), window, cx));
        cx.subscribe(
            &modal,
            |this, _, event: &SettingsModalEvent, cx| match event {
                SettingsModalEvent::Close => {
                    this.settings = None;
                    cx.notify();
                }
            },
        )
        .detach();
        self.settings = Some(modal);
        cx.notify();
    }

    fn on_open_search(&mut self, _: &OpenSearch, window: &mut Window, cx: &mut Context<Self>) {
        let search = cx.new(|cx| CommandSearch::new(window, cx));
        cx.subscribe(
            &search,
            |this, _, event: &CommandSearchEvent, cx| match event {
                CommandSearchEvent::Selected(id) => {
                    this.set_view(PanelView::Home, cx);
                    let id = *id;
                    this.history.update(cx, |h, cx| h.highlight(id, cx));
                }
            },
        )
        .detach();
        super::overlays::show_custom(search.into(), cx);
    }

    fn on_open_settings(&mut self, _: &OpenSettings, window: &mut Window, cx: &mut Context<Self>) {
        self.open_settings(None, window, cx);
    }

    fn on_close_overlay(&mut self, _: &CloseOverlay, _window: &mut Window, cx: &mut Context<Self>) {
        let overlays = Overlays::global(cx);
        if overlays.read(cx).has_dialog() {
            overlays.update(cx, |o, cx| o.close_dialog(cx));
        } else if overlays.read(cx).has_custom() {
            overlays.update(cx, |o, cx| o.close_custom(cx));
        } else if let Some(modal) = &self.settings {
            modal.update(cx, |m, cx| m.request_close(cx));
        }
    }

    pub fn set_view(&mut self, view: PanelView, cx: &mut Context<Self>) {
        self.active = view;
        cx.notify();
    }

    fn toggle_sidebar(&mut self, cx: &mut Context<Self>) {
        self.peek_close = None;
        self.peek = false;
        self.sidebar_collapsed = !self.sidebar_collapsed;
        self.suppress_peek = self.sidebar_collapsed;
        let collapsed = self.sidebar_collapsed;
        AppState::global(cx).update(cx, |s, cx| {
            s.update_settings(cx, |s| s.sidebar_collapsed = collapsed)
        });
        cx.notify();
    }

    fn show_peek(&mut self, cx: &mut Context<Self>) {
        if self.suppress_peek || !self.sidebar_collapsed {
            return;
        }
        self.peek_close = None;
        if !self.peek {
            self.peek = true;
            cx.notify();
        }
    }

    fn hide_peek(&mut self, cx: &mut Context<Self>) {
        if !self.peek {
            return;
        }
        self.peek_close = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(PEEK_CLOSE_DELAY).await;
            let _ = this.update(cx, |this, cx| {
                this.peek = false;
                cx.notify();
            });
        }));
    }

    fn leave_toggle(&mut self, cx: &mut Context<Self>) {
        self.suppress_peek = false;
        self.hide_peek(cx);
    }

    fn render_sidebar(&mut self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let p = cx.palette();
        let active = self.active;
        let mut nav = div()
            .flex()
            .flex_col()
            .gap(px(2.))
            .px(px(8.))
            .pt(px(8.))
            .pb(px(8.));
        for view in PanelView::ALL {
            let is_active = view == active;
            let icon_color = if is_active {
                p.primary
            } else if p.dark {
                p.foreground.opacity(0.65)
            } else {
                p.foreground.opacity(0.7)
            };
            nav = nav.child(
                div()
                    .id(SharedString::from(format!("nav-{view:?}")))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .w_full()
                    .h(px(32.))
                    .px(px(10.))
                    .rounded(px(6.))
                    .cursor_pointer()
                    .when(is_active, |d| {
                        d.bg(if p.dark {
                            p.white.opacity(0.06)
                        } else {
                            p.primary.opacity(0.08)
                        })
                    })
                    .when(!is_active, |d| d.hover(|s| s.bg(p.hover_wash(0.04))))
                    .child(icon(view.icon(), 16., icon_color))
                    .child(
                        div()
                            .text_size(px(TEXT_BODY))
                            .when(is_active, |d| d.font_weight(FontWeight::MEDIUM))
                            .text_color(if is_active {
                                p.foreground
                            } else {
                                p.foreground.opacity(if p.dark { 0.85 } else { 0.9 })
                            })
                            .child(view.label()),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.set_view(view, cx))),
            );
        }

        div()
            .w(px(SIDEBAR_WIDTH))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .bg(p.surface_window)
            .child(drag_region().w_full().h(px(40.)).flex_none())
            .child(nav)
            .child(div().flex_1())
            .child(
                div().px(px(8.)).pb(px(8.)).child(
                    div()
                        .id("nav-settings")
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .w_full()
                        .h(px(32.))
                        .px(px(10.))
                        .rounded(px(6.))
                        .cursor_pointer()
                        .hover(|s| s.bg(p.hover_wash(0.04)))
                        .child(icon(
                            "settings",
                            16.,
                            p.foreground.opacity(if p.dark { 0.65 } else { 0.7 }),
                        ))
                        .child(
                            div()
                                .text_size(px(TEXT_BODY))
                                .text_color(p.foreground.opacity(if p.dark { 0.85 } else { 0.9 }))
                                .child(t("sidebar.settings")),
                        )
                        .on_click(cx.listener(|_, _, window, cx| {
                            window.dispatch_action(Box::new(OpenSettings), cx)
                        })),
                ),
            )
    }

    fn render_top_bar(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let p = cx.palette();
        let clear_traffic_lights = self.sidebar_collapsed;
        // Eased with the sidebar so the toggle glides past the traffic lights.
        let start_pad = f32_tx(
            "topbar-start-pad",
            if clear_traffic_lights { 76. } else { 12. },
            DUR_SHEET,
            ease_out,
            window,
            cx,
        );
        let collapsed = self.sidebar_collapsed;
        let title = self.active.label();

        div()
            .h(px(48.))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(16.))
            .pl(px(start_pad))
            .pr(px(12.))
            .border_b_1()
            .border_color(if p.dark {
                p.white.opacity(0.1)
            } else {
                p.border
            })
            .on_mouse_down(MouseButton::Left, |event, window, _| {
                if event.click_count == 1 {
                    window.start_window_move();
                }
            })
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(
                        div()
                            .id("sidebar-toggle")
                            .flex()
                            .flex_none()
                            .items_center()
                            .justify_center()
                            .size(px(32.))
                            .rounded(px(8.))
                            .cursor_pointer()
                            .hover(|s| s.bg(p.hover_wash(0.05)))
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .child(icon(
                                "panel-left-close",
                                16.,
                                p.foreground.opacity(if p.dark { 0.5 } else { 0.6 }),
                            ))
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_sidebar(cx)))
                            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                                if !collapsed {
                                    return;
                                }
                                if *hovered {
                                    this.show_peek(cx);
                                } else {
                                    this.leave_toggle(cx);
                                }
                            })),
                    )
                    .child(
                        div()
                            .text_size(px(TEXT_TITLE))
                            .font_weight(FontWeight::SEMIBOLD)
                            .line_height(px(LINE_TITLE))
                            .text_color(p.foreground)
                            .truncate()
                            .child(title),
                    ),
            )
            .child(
                div()
                    .id("search-trigger")
                    .flex_none()
                    .w(px(340.))
                    .max_w(relative(0.34))
                    .h(px(32.))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .px(px(16.))
                    .rounded(px(6.))
                    .border_1()
                    .border_color(if p.dark {
                        p.white.opacity(0.1)
                    } else {
                        p.border
                    })
                    .bg(if p.dark {
                        p.white.opacity(0.05)
                    } else {
                        p.foreground.opacity(0.04)
                    })
                    .hover(|s| {
                        s.bg(if p.dark {
                            p.white.opacity(0.08)
                        } else {
                            p.foreground.opacity(0.06)
                        })
                    })
                    .cursor_pointer()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(icon("search", 14., p.muted_foreground.opacity(0.7)))
                    .child(
                        div()
                            .flex_1()
                            .truncate()
                            .text_size(px(TEXT_BODY))
                            .text_color(p.muted_foreground.opacity(0.7))
                            .child(t("commandSearch.sections.transcripts")),
                    )
                    .child(
                        div()
                            .flex_none()
                            .font_family(FONT_MONO)
                            .text_size(px(TEXT_CAPTION))
                            .text_color(p.muted_foreground.opacity(0.7))
                            .child("⌘K"),
                    )
                    .on_click(|_, window, cx| window.dispatch_action(Box::new(OpenSearch), cx)),
            )
            .child(div().flex_1())
    }

    fn render_content(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        match self.active {
            PanelView::Home => self.history.clone().into_any_element(),
            PanelView::Insights => self
                .insights
                .get_or_insert_with(|| cx.new(InsightsView::new))
                .clone()
                .into_any_element(),
            PanelView::Dictionary => self
                .dictionary
                .get_or_insert_with(|| cx.new(|cx| DictionaryView::new(window, cx)))
                .clone()
                .into_any_element(),
            PanelView::Upload => self.upload.clone().into_any_element(),
        }
    }
}

/// An area that moves the window when dragged, like `-webkit-app-region: drag`.
pub fn drag_region() -> Div {
    div().on_mouse_down(MouseButton::Left, |event, window, _| {
        if event.click_count == 1 {
            window.start_window_move();
        }
    })
}

impl Focusable for ControlPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for ControlPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = cx.palette();
        let collapsed = self.sidebar_collapsed;
        let spacer = f32_tx(
            "sidebar-spacer",
            if collapsed { 0. } else { SIDEBAR_WIDTH },
            DUR_SHEET,
            ease_out,
            window,
            cx,
        );
        let visible = !collapsed || self.peek;
        let offset = f32_tx(
            "sidebar-offset",
            if visible { 0. } else { -SIDEBAR_WIDTH },
            DUR_SHEET,
            ease_out,
            window,
            cx,
        );
        let peeking = collapsed && self.peek;
        let sidebar = self.render_sidebar(cx);
        let top_bar = self.render_top_bar(window, cx);
        // Each view eases in 3 px as it replaces the last.
        // Insights fills the viewport, so its wrapper must too.
        let fill = self.active == PanelView::Insights;
        let content = div()
            .relative()
            .when(fill, |d| d.h_full())
            .child(self.render_content(window, cx))
            .with_animation(
                ("view-in", self.active as usize),
                Animation::new(Duration::from_millis(DUR_BASE)).with_easing(ease_out),
                |d, t| d.opacity(t).top(px(3. * (1. - t))),
            );
        let scrolled = self.scroll.offset().y < px(-4.);
        let fade = f32_tx(
            "scroll-fade",
            if scrolled { 1. } else { 0. },
            if scrolled { DUR_SNAP } else { DUR_FAST },
            ease_snap,
            window,
            cx,
        );
        let settings_modal = self.settings.clone();
        let custom = Overlays::global(cx).read(cx).custom();
        let dialog = render_dialog(window, cx);
        let toasts = render_toasts(window, cx);

        div()
            .key_context("ControlPanel")
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::on_open_settings))
            .on_action(cx.listener(Self::on_open_search))
            .on_action(cx.listener(Self::on_close_overlay))
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .bg(p.surface_window)
            .font_family(FONT_UI)
            .text_color(p.foreground)
            .child(
                div()
                    .relative()
                    .flex_1()
                    .flex()
                    .overflow_hidden()
                    .child(div().flex_none().w(px(spacer)))
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .overflow_hidden()
                            .p(px(8.))
                            .child(
                                div()
                                    .flex_1()
                                    .min_h_0()
                                    .flex()
                                    .flex_col()
                                    .overflow_hidden()
                                    .rounded(px(12.))
                                    .border_1()
                                    .border_color(p.border)
                                    .bg(p.background)
                                    .child(top_bar)
                                    .child(
                                        div()
                                            .relative()
                                            .flex_1()
                                            .min_h_0()
                                            .flex()
                                            .flex_col()
                                            .child(
                                                div()
                                                    .id("panel-scroll")
                                                    .flex_1()
                                                    .min_h_0()
                                                    .overflow_y_scroll()
                                                    .track_scroll(&self.scroll)
                                                    .child(content),
                                            )
                                            // Content scrolled under the top bar fades out
                                            // instead of meeting its border at a hard edge.
                                            .when(fade > 0.001, |d| {
                                                d.child(
                                                    div()
                                                        .absolute()
                                                        .top_0()
                                                        .left_0()
                                                        .right(px(12.))
                                                        .h(px(SCROLL_FADE))
                                                        .opacity(fade)
                                                        .bg(linear_gradient(
                                                            180.,
                                                            linear_color_stop(p.background, 0.),
                                                            linear_color_stop(
                                                                p.background.opacity(0.),
                                                                1.,
                                                            ),
                                                        )),
                                                )
                                            }),
                                    ),
                            ),
                    )
                    // Painted after the main column so the peeking sidebar
                    // overlays it.
                    .child(
                        div()
                            .absolute()
                            .top_0()
                            .bottom_0()
                            .left(px(offset))
                            .when(peeking, |d| {
                                d.shadow(vec![BoxShadow {
                                    color: hsla(0., 0., 0., 0.2),
                                    offset: point(px(10.), px(0.)),
                                    blur_radius: px(40.),
                                    spread_radius: px(-18.),
                                    inset: false,
                                }])
                            })
                            .id("sidebar-host")
                            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                                if !this.sidebar_collapsed {
                                    return;
                                }
                                if *hovered {
                                    this.show_peek(cx);
                                } else {
                                    this.hide_peek(cx);
                                }
                            }))
                            .child(sidebar),
                    ),
            )
            .when_some(settings_modal, |d, modal| d.child(modal))
            .children(custom)
            .children(dialog)
            .children(toasts)
    }
}
