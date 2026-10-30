//! The main window: a collapsible sidebar split from the canvas by a hairline,
//! a top bar with the transcript search, and the active view.

use std::time::Duration;

use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::i18n::t;

use crate::app_state::AppState;

use super::command_search::{CommandSearch, CommandSearchEvent};
use super::components::{NavSurface, drop, f32_tx, icon, inset, kbd, nav_row};
use super::dictionary_view::DictionaryView;
use super::history_view::HistoryView;
use super::insights_view::InsightsView;
use super::overlays::{Overlays, render_dialog, render_toasts};
use super::settings_modal::{SettingsModal, SettingsModalEvent, SettingsSection};
use super::theme::{
    CONTROL_HEIGHT, DUR_BASE, DUR_FAST, DUR_SHEET, DUR_SNAP, FONT_UI, PaletteExt, RADIUS_LG,
    TEXT_BODY, WORKSPACE_TOP_BAR_HEIGHT, ease_out, ease_snap,
};
use super::upload_view::{UploadEvent, UploadView};

actions!(control_panel, [OpenSearch, OpenSettings, CloseOverlay]);

pub const SIDEBAR_WIDTH: f32 = 232.;
/// Where the top bar's controls start when the sidebar is hidden, clear of
/// the traffic lights.
const TRAFFIC_LIGHTS_END: f32 = 84.;
/// Where the sidebar's app name starts: the traffic lights plus a gap.
const BRAND_INSET: f32 = 96.;
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
        // Closing an overlay drops the input that held focus. Without a focused
        // element, actions never reach this view's listeners, so take focus back.
        cx.on_focus_lost(window, |this, window, cx| window.focus(&this.focus, cx))
            .detach();
        let overlays = Overlays::global(cx);
        cx.observe(&overlays, |_, _, cx| cx.notify()).detach();
        let scroll = ScrollHandle::new();
        let history = {
            let scroll = scroll.clone();
            cx.new(|cx| HistoryView::new(scroll, cx))
        };
        let upload = cx.new(UploadView::new);
        cx.subscribe_in(
            &upload,
            window,
            |this, _, event: &UploadEvent, window, cx| match event {
                UploadEvent::OpenHistory => this.set_view(PanelView::Home, cx),
                UploadEvent::OpenSettings(section) => {
                    this.open_settings(Some(*section), window, cx)
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
            scroll,
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
                // Closed here, not by the palette: dropping it before the event
                // is delivered would drop this listener with it.
                CommandSearchEvent::Selected(id) => {
                    super::overlays::close_custom(cx);
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
        let mut nav = div().flex().flex_col().gap(px(2.)).px(px(8.)).pt(px(4.));
        for view in PanelView::ALL {
            nav = nav.child(
                nav_row(
                    SharedString::from(format!("nav-{view:?}")),
                    view.icon(),
                    view.label(),
                    view == active,
                    NavSurface::App,
                    &p,
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
            .border_r_1()
            .border_color(p.border)
            .child(
                drag_region()
                    .w_full()
                    .h(px(WORKSPACE_TOP_BAR_HEIGHT))
                    .flex_none()
                    .flex()
                    .items_center()
                    .pl(px(BRAND_INSET))
                    .child(
                        div()
                            .text_size(px(TEXT_BODY))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(p.foreground)
                            .child("Whisper"),
                    ),
            )
            .child(nav)
            .child(div().flex_1())
            .child(
                div().px(px(8.)).pb(px(8.)).child(
                    nav_row(
                        "nav-settings".into(),
                        "settings",
                        t("sidebar.settings"),
                        false,
                        NavSurface::App,
                        &p,
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
            if clear_traffic_lights {
                TRAFFIC_LIGHTS_END - 4.
            } else {
                12.
            },
            DUR_SHEET,
            ease_out,
            window,
            cx,
        );
        let collapsed = self.sidebar_collapsed;
        let title = self.active.label();

        div()
            .h(px(WORKSPACE_TOP_BAR_HEIGHT))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(12.))
            .pl(px(start_pad))
            .pr(px(12.))
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
                            .size(px(28.))
                            .rounded(px(RADIUS_LG))
                            .cursor_pointer()
                            .hover(|s| s.bg(p.hover_wash(0.05)))
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .child(icon("panel-left-close", 16., p.muted_foreground))
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
                            .text_size(px(TEXT_BODY))
                            .font_weight(FontWeight::MEDIUM)
                            .line_height(px(20.))
                            .text_color(p.foreground)
                            .truncate()
                            .child(title),
                    ),
            )
            .child(
                div()
                    .id("search-trigger")
                    .flex_none()
                    .w(px(360.))
                    .max_w(relative(0.34))
                    .h(px(CONTROL_HEIGHT))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .pl(px(10.))
                    .pr(px(6.))
                    .rounded(px(RADIUS_LG))
                    .border_1()
                    .border_color(p.input_border)
                    .bg(if p.dark {
                        p.white.opacity(0.035)
                    } else {
                        p.popover
                    })
                    .shadow(if p.dark {
                        vec![inset(0., 1., 0., p.white.opacity(0.05))]
                    } else {
                        vec![drop(0., 1., 2., 0.05)]
                    })
                    .hover(|s| {
                        s.bg(if p.dark {
                            p.white.opacity(0.06)
                        } else {
                            p.muted.opacity(0.6)
                        })
                    })
                    .cursor_pointer()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(icon("search", 16., p.muted_foreground))
                    .child(
                        div()
                            .flex_1()
                            .truncate()
                            .text_size(px(TEXT_BODY))
                            .text_color(p.muted_foreground)
                            .child(t("commandSearch.sections.transcripts")),
                    )
                    .child(kbd("⌘K", &p))
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
        let dialog = render_dialog(cx);
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
                        div().flex_1().flex().flex_col().overflow_hidden().child(
                            div()
                                .flex_1()
                                .min_h_0()
                                .flex()
                                .flex_col()
                                .overflow_hidden()
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
