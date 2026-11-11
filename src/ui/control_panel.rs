//! The main window: a collapsible sidebar split from the canvas by a hairline,
//! a top bar with the transcript search, and the active view.

use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::i18n::t;

use crate::app_state::AppState;

use super::command_search::{CommandSearch, CommandSearchEvent};
use super::components::{
    Button, ButtonSize, ButtonVariant, KeyboardFocusable, MenuTriggerBounds, NavSurface, drop,
    f32_tx, icon, inset, kbd, nav_row, scroll_thumb, text_tooltip,
};
use super::dictionary_view::DictionaryView;
use super::history_view::HistoryView;
use super::insights_view::InsightsView;
use super::menu::{MenuAppearance, MenuFocus, MenuPlacement, PopupMenu};
use super::overlays::{Overlays, render_dialog, render_toasts};
use super::settings_modal::{SettingsModal, SettingsModalEvent, SettingsSection};
use super::sidebar_icon::SidebarIcon;
use super::theme::{
    CONTROL_HEIGHT, DUR_BASE, DUR_FAST, DUR_SNAP, FONT_UI, PaletteExt, RADIUS_LG, RADIUS_MD,
    TEXT_BODY, WORKSPACE_TOP_BAR_HEIGHT, ease_css, ease_out, ease_snap, motion,
};
use super::upload_view::{UploadEvent, UploadStore};

actions!(control_panel, [OpenSearch, OpenSettings, CloseOverlay]);

pub const SIDEBAR_WIDTH: f32 = 232.;
/// Separate window controls from app navigation, with a fixed toggle in both states.
const SIDEBAR_TOGGLE_LEFT: f32 = 88.;
const SIDEBAR_TOGGLE_SIZE: f32 = 28.;
const BRAND_INSET: f32 = SIDEBAR_TOGGLE_LEFT + SIDEBAR_TOGGLE_SIZE + 12.;
const COLLAPSED_TITLE_INSET: f32 = BRAND_INSET;
const SIDEBAR_DURATION: u64 = 200;
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
            Self::Home => "clock",
            Self::Insights => "bar-chart3",
            Self::Upload => "upload",
            Self::Dictionary => "book-open",
        }
    }
}

pub struct ControlPanel {
    pub active: PanelView,
    sidebar_collapsed: bool,
    sidebar_icon: SidebarIcon,
    focus: FocusHandle,
    history: Entity<HistoryView>,
    insights: Option<Entity<InsightsView>>,
    dictionary: Option<Entity<DictionaryView>>,
    upload: Entity<UploadStore>,
    settings: Option<Entity<SettingsModal>>,
    history_after_settings: Option<i64>,
    scroll: ScrollHandle,
    /// Whether the History page's ⋯ menu in the top bar is open.
    page_menu_open: bool,
    /// The page whose scroll extent has been measured; a newly shown page
    /// renders once more so its scroll thumb and fade show from the start.
    measured_view: Option<PanelView>,
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
        let upload = UploadStore::global(cx);
        cx.subscribe_in(
            &upload,
            window,
            |this, _, event: &UploadEvent, window, cx| match event {
                UploadEvent::OpenHistory => this.set_view(PanelView::Home, cx),
                UploadEvent::RevealHistory(id) => this.reveal_history(*id, cx),
                UploadEvent::OpenSettings(section) => {
                    this.open_settings(Some(*section), window, cx)
                }
            },
        )
        .detach();
        #[allow(unused_mut)]
        let mut this = Self {
            active: PanelView::Home,
            sidebar_collapsed: collapsed,
            sidebar_icon: SidebarIcon::new(collapsed),
            focus,
            history,
            insights: None,
            dictionary: None,
            upload,
            settings: None,
            history_after_settings: None,
            scroll,
            page_menu_open: false,
            measured_view: None,
        };
        #[cfg(debug_assertions)]
        this.apply_dev_scene(window, cx);
        this
    }

    #[cfg(debug_assertions)]
    fn apply_dev_scene(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        use super::dev_scene::{Scene, scene};
        match scene() {
            Some(Scene::View(view)) => self.active = view,
            Some(Scene::Settings(section)) => self.open_settings(Some(section), window, cx),
            Some(Scene::Search) => {
                window.on_next_frame(|window, cx| window.dispatch_action(Box::new(OpenSearch), cx))
            }
            _ => {}
        }
    }

    pub fn settings_open(&self) -> bool {
        self.settings.is_some()
    }

    pub fn can_close(&mut self, cx: &mut Context<Self>) -> bool {
        self.settings.as_ref().is_none_or(|modal| {
            modal.update(cx, |m, cx| {
                m.confirm_prompt_close(crate::app::close_control_panel, cx)
            })
        })
    }

    pub fn prepare_close(&mut self, cx: &mut Context<Self>) {
        self.history.update(cx, |h, cx| h.commit_delete(cx));
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
        cx.subscribe_in(
            &modal,
            window,
            |this, _, event: &SettingsModalEvent, window, cx| match event {
                SettingsModalEvent::Close(restore_focus) => {
                    this.settings = None;
                    if let Some(id) = this.history_after_settings.take() {
                        this.history.update(cx, |h, cx| h.highlight(id, cx));
                    } else if let Some(handle) = restore_focus.clone() {
                        // Restore after the sheet's focus trap leaves the rendered tree.
                        window.on_next_frame(move |window, cx| window.focus(&handle, cx));
                    }
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
                    this.reveal_history(*id, cx);
                }
            },
        )
        .detach();
        super::overlays::show_custom(search.into(), cx);
    }

    fn on_open_settings(&mut self, _: &OpenSettings, window: &mut Window, cx: &mut Context<Self>) {
        self.open_settings(None, window, cx);
    }

    fn reveal_history(&mut self, id: i64, cx: &mut Context<Self>) {
        self.set_view(PanelView::Home, cx);
        if let Some(modal) = &self.settings {
            // Start the spotlight only after the sheet's close transition ends.
            self.history_after_settings = Some(id);
            modal.update(cx, |m, cx| m.request_close(cx));
        } else {
            self.history.update(cx, |h, cx| h.highlight(id, cx));
        }
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
        if view == PanelView::Insights
            && let Some(insights) = &self.insights
        {
            insights.update(cx, |insights, cx| insights.refresh_for_date(cx));
        }
        cx.notify();
    }

    fn toggle_sidebar(&mut self, cx: &mut Context<Self>) {
        self.sidebar_collapsed = !self.sidebar_collapsed;
        let collapsed = self.sidebar_collapsed;
        AppState::global(cx).update(cx, |s, cx| {
            s.update_settings(cx, |s| s.sidebar_collapsed = collapsed)
        });
        cx.notify();
    }

    fn render_sidebar_toggle(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let p = cx.palette();
        let label = t(if self.sidebar_collapsed {
            "sidebar.show"
        } else {
            "sidebar.hide"
        });
        div()
            .id("sidebar-toggle")
            .keyboard_focusable("panel-sidebar-toggle", Role::Button, label.clone(), cx)
            .tooltip(text_tooltip(label))
            .absolute()
            .left(px(SIDEBAR_TOGGLE_LEFT))
            .top(px((WORKSPACE_TOP_BAR_HEIGHT - SIDEBAR_TOGGLE_SIZE) / 2.))
            .flex()
            .items_center()
            .justify_center()
            .size(px(SIDEBAR_TOGGLE_SIZE))
            .rounded(px(RADIUS_LG))
            .cursor_pointer()
            .hover(|s| s.bg(p.hover_wash(0.05)))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(self.sidebar_icon.render(p.muted_foreground))
            .on_click(cx.listener(|this, _, _, cx| this.toggle_sidebar(cx)))
    }

    fn render_sidebar(
        &mut self,
        sidebar_width: f32,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
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
                    cx,
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
                    // A stationary clipping slot lets the brand slide with navigation
                    // without painting over the fixed window controls or toggle.
                    .pl(px(BRAND_INSET + SIDEBAR_WIDTH - sidebar_width))
                    .child(
                        div()
                            .flex_none()
                            .w(px(SIDEBAR_WIDTH - BRAND_INSET))
                            .h(px(20.))
                            .overflow_hidden()
                            .child(
                                div()
                                    .relative()
                                    .left(px(sidebar_width - SIDEBAR_WIDTH))
                                    .text_size(px(TEXT_BODY))
                                    .line_height(px(crate::ui::theme::LINE_BODY))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(p.foreground)
                                    .child("Whisper"),
                            ),
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
                        cx,
                    )
                    .on_click(cx.listener(|_, _, window, cx| {
                        window.dispatch_action(Box::new(OpenSettings), cx)
                    })),
                ),
            )
    }

    fn render_top_bar(
        &mut self,
        sidebar_width: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let p = cx.palette();
        // Derive the title's inset from the same reveal as the sidebar.
        let start_pad = 12. + (COLLAPSED_TITLE_INSET - 12.) * (1. - sidebar_width / SIDEBAR_WIDTH);
        let title = self.active.label();
        // Page-level actions sit at the top bar's end; Home's is Clear History.
        let page_actions =
            self.active == PanelView::Home && !AppState::global(cx).read(cx).history.is_empty();
        let full_search = self.active == PanelView::Home;
        // Page menus share the top bar's trailing slot on every page. The
        // view is created here when needed, so its menu shows on the first
        // frame rather than one frame after the page.
        let dictionary_menu = (self.active == PanelView::Dictionary).then(|| {
            self.dictionary
                .get_or_insert_with(|| cx.new(|cx| DictionaryView::new(window, cx)))
                .clone()
                .update(cx, |d, cx| d.render_page_menu(window, cx))
        });
        let search_field = div()
            .id("search-trigger")
            .keyboard_focusable(
                "panel-search-trigger",
                Role::Button,
                format!("{} (⌘K)", t("commandSearch.placeholder")),
                cx,
            )
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
            .border_color(p.control_border)
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
                    .child(t("commandSearch.placeholder")),
            )
            .child(kbd("⌘K", &p))
            .on_click(|_, window, cx| window.dispatch_action(Box::new(OpenSearch), cx));

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
                            .text_size(px(TEXT_BODY))
                            .font_weight(FontWeight::SEMIBOLD)
                            .line_height(px(crate::ui::theme::LINE_BODY))
                            .text_color(p.foreground)
                            .truncate()
                            .child(title),
                    ),
            )
            // History, the page it searches, shows the full field; elsewhere
            // a compact button keeps ⌘K at hand without a second search box
            // competing with the page's own filter.
            .when(full_search, |d| d.child(search_field))
            .child(
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_end()
                    .gap(px(4.))
                    .when(!full_search, |d| {
                        d.child(
                            div()
                                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                                .child(
                                    Button::new("search-compact")
                                        .icon("search", 15.)
                                        .variant(ButtonVariant::Ghost)
                                        .size(ButtonSize::Sm)
                                        .aria_label(format!(
                                            "{} (⌘K)",
                                            t("commandSearch.placeholder")
                                        ))
                                        .on_click(|_, window, cx| {
                                            window.dispatch_action(Box::new(OpenSearch), cx)
                                        }),
                                ),
                        )
                    })
                    .children(dictionary_menu)
                    .when(page_actions, |d| {
                        let trigger = MenuTriggerBounds::default();
                        d.child(
                            div()
                                .relative()
                                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                                .child(
                                    div().relative().child(trigger.measure()).child(
                                        Button::new("history-page-menu")
                                            .icon("more-vertical", 15.)
                                            .variant(ButtonVariant::Ghost)
                                            .size(ButtonSize::Sm)
                                            .aria_label(t("controlPanel.history.pageActions"))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.toggle_page_menu(cx)
                                            })),
                                    ),
                                )
                                .when(self.page_menu_open, |d| {
                                    d.child(self.render_page_menu(trigger, window, cx))
                                }),
                        )
                    }),
            )
    }

    fn toggle_page_menu(&mut self, cx: &mut Context<Self>) {
        self.page_menu_open = !self.page_menu_open;
        cx.notify();
    }

    fn close_page_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.page_menu_open = false;
        MenuFocus::new(
            ["history-page-clear".into()],
            "history-page-menu".into(),
            cx,
        )
        .restore(false, window, cx);
        cx.notify();
    }

    /// The History page's ⋯ menu. Its one item is destructive, so it sits
    /// behind the menu rather than as a bare button in the toolbar.
    fn render_page_menu(
        &mut self,
        trigger: MenuTriggerBounds,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = cx.palette();
        let label = t("controlPanel.history.clearHistory");
        let item = div()
            .id("history-page-clear")
            .keyboard_focusable("history-page-clear", Role::MenuItem, label.clone(), cx)
            .flex()
            .items_center()
            .gap(px(10.))
            .px(px(8.))
            .py(px(6.))
            .rounded(px(RADIUS_MD))
            .text_size(px(TEXT_BODY))
            .text_color(p.destructive)
            .cursor_pointer()
            .hover(move |s| s.bg(p.destructive.opacity(0.08)))
            .child(icon("trash2", 14., p.destructive))
            .child(label)
            .on_click(cx.listener(|this, _, window, cx| {
                this.close_page_menu(window, cx);
                HistoryView::clear_all(cx);
            }));
        PopupMenu {
            id: "history-page-menu-list".into(),
            min_width: 200.,
            appearance: MenuAppearance::History,
            placement: MenuPlacement::TriggerRight,
            trigger: Some(trigger),
            focus: MenuFocus::new(
                ["history-page-clear".into()],
                "history-page-menu".into(),
                cx,
            ),
        }
        .render(
            [item.into_any_element()],
            |this, _, cx| {
                this.page_menu_open = false;
                cx.notify();
            },
            window,
            cx,
        )
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
        super::components::prune_focus_handles(cx);
        let p = cx.palette();
        let settings_modal = self.settings.clone();
        let custom = Overlays::global(cx).read(cx).custom();
        let dialog = render_dialog(window, cx);
        let toasts = render_toasts(window, cx);
        let root = div()
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
            .text_color(p.foreground);
        // Occlusion only blocks hit testing. Without this branch, every settings
        // scroll still lays out the entire History page behind the opaque sheet.
        // Keep the view entities and scroll handle alive so closing restores them.
        if let Some(modal) = &settings_modal
            && modal.read(cx).covers_workspace(window.viewport_size())
        {
            return root
                .child(modal.clone())
                .children(custom)
                .children(dialog)
                .children(toasts)
                .into_any_element();
        }
        let collapsed = self.sidebar_collapsed;
        let icon_animating = self.sidebar_icon.update(collapsed, cx.reduce_motion());
        // One reveal drives both clipping and content layout, including reversals.
        let sidebar_width = f32_tx(
            "sidebar-reveal",
            if collapsed { 0. } else { SIDEBAR_WIDTH },
            SIDEBAR_DURATION,
            ease_css,
            window,
            cx,
        );
        let target_width = if collapsed { 0. } else { SIDEBAR_WIDTH };
        if sidebar_width != target_width || icon_animating {
            // Keep view-owned layout and icon physics advancing through retained frames.
            cx.on_next_frame(window, |_, _, cx| cx.notify());
        }
        let sidebar = (sidebar_width > 0.).then(|| self.render_sidebar(sidebar_width, cx));
        let top_bar = self.render_top_bar(sidebar_width, window, cx);
        let sidebar_toggle = self.render_sidebar_toggle(cx);
        // Each view eases in 3 px as it replaces the last.
        // Insights fills the viewport, so its wrapper must too.
        let fill = self.active == PanelView::Insights;
        let content = div()
            .relative()
            // At least a viewport tall so short states can center, but free to
            // grow: a fixed height would leave the scroll area nothing to scroll.
            .when(fill, |d| d.min_h_full())
            .child(self.render_content(window, cx))
            .with_animation(
                ("view-in", self.active as usize),
                motion(DUR_BASE, cx).with_easing(ease_out),
                |d, t| d.opacity(t).top(px(3. * (1. - t))),
            );
        let scrolled = self.scroll.offset().y < px(-4.);
        let max_scroll = self.scroll.max_offset().y;
        let more_below = max_scroll > px(1.) && self.scroll.offset().y > -max_scroll + px(1.);
        let bottom_fade = f32_tx(
            "scroll-fade-bottom",
            if more_below { 1. } else { 0. },
            DUR_FAST,
            ease_snap,
            window,
            cx,
        );
        let thumb = scroll_thumb(&self.scroll);
        if self.measured_view != Some(self.active) {
            self.measured_view = Some(self.active);
            cx.on_next_frame(window, |_, _, cx| cx.notify());
        }
        let fade = f32_tx(
            "scroll-fade",
            if scrolled { 1. } else { 0. },
            if scrolled { DUR_SNAP } else { DUR_FAST },
            ease_snap,
            window,
            cx,
        );
        root.child(
            div()
                .relative()
                .flex_1()
                .flex()
                .overflow_hidden()
                .child(
                    div()
                        .id("sidebar-host")
                        .relative()
                        .flex_none()
                        .w(px(sidebar_width))
                        .h_full()
                        .overflow_hidden()
                        .children(sidebar.map(|sidebar| {
                            div()
                                .absolute()
                                .inset_0()
                                .left(px(sidebar_width - SIDEBAR_WIDTH))
                                .child(sidebar)
                        })),
                )
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
                                                    linear_color_stop(p.background.opacity(0.), 1.),
                                                )),
                                        )
                                    })
                                    // A quiet thumb and a short bottom fade say a long
                                    // page continues, as the Settings sheet does.
                                    .when_some(thumb, |d, (top, height)| {
                                        d.child(
                                            div()
                                                .absolute()
                                                .right(px(3.))
                                                .top(top)
                                                .w(px(4.))
                                                .h(height)
                                                .rounded_full()
                                                .bg(p.foreground.opacity(if p.dark {
                                                    0.22
                                                } else {
                                                    0.18
                                                })),
                                        )
                                    })
                                    .when(bottom_fade > 0.01, |d| {
                                        d.child(
                                            div()
                                                .absolute()
                                                .bottom_0()
                                                .left_0()
                                                .right(px(12.))
                                                .h(px(16.))
                                                .opacity(bottom_fade)
                                                .bg(linear_gradient(
                                                    0.,
                                                    linear_color_stop(p.background, 0.),
                                                    linear_color_stop(p.background.opacity(0.), 1.),
                                                )),
                                        )
                                    }),
                            ),
                    ),
                ),
        )
        .child(sidebar_toggle)
        .when_some(settings_modal, |d, modal| d.child(modal))
        .children(custom)
        .children(dialog)
        .children(toasts)
        .into_any_element()
    }
}
