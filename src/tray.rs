//! The menu bar icon: start/stop listening, show or
//! hide the control panel, quit. Shown only while "Show in menu bar" is on.

use std::cell::RefCell;

use gpui_kit::*;
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

use whisper::i18n::t;

use crate::app_state::{AppEvent, AppState};
use crate::dictation::Dictation;

const TOGGLE: &str = "toggle-listening";
const PANEL: &str = "toggle-panel";
const QUIT: &str = "quit";

thread_local! {
    static TRAY: RefCell<Option<TrayIcon>> = const { RefCell::new(None) };
}

fn icon() -> Option<Icon> {
    let bytes = crate::ui::assets::bytes("iconTemplate@3x.png")?;
    let image = image::load_from_memory(&bytes).ok()?.into_rgba8();
    let (w, h) = image.dimensions();
    Icon::from_rgba(image.into_raw(), w, h).ok()
}

fn control_panel_visible(cx: &App) -> bool {
    cx.windows()
        .iter()
        .any(|w| w.downcast::<gpui_kit::base::Root>().is_some())
}

fn build_menu(cx: &App) -> Menu {
    let recording = AppState::global(cx).read(cx).dictation.is_recording;
    let menu = Menu::new();
    let _ = menu.append(&MenuItem::with_id(
        TOGGLE,
        if recording {
            t("app.commandMenu.stopListening")
        } else {
            t("app.commandMenu.startListening")
        },
        true,
        None,
    ));
    let _ = menu.append(&PredefinedMenuItem::separator());
    let _ = menu.append(&MenuItem::with_id(
        PANEL,
        if control_panel_visible(cx) {
            t("tray.hideControlPanel")
        } else {
            t("tray.openControlPanel")
        },
        true,
        None,
    ));
    let _ = menu.append(&PredefinedMenuItem::separator());
    let _ = menu.append(&MenuItem::with_id(QUIT, t("tray.quit"), true, None));
    menu
}

fn sync(cx: &mut App) {
    let visible = AppState::settings(cx).show_menu_bar_icon;
    TRAY.with(|tray| {
        let mut tray = tray.borrow_mut();
        if !visible {
            *tray = None;
            return;
        }
        if let Some(existing) = tray.as_ref() {
            existing.set_menu(Some(Box::new(build_menu(cx))));
            let _ = existing.set_tooltip(Some(t("tray.tooltip")));
            return;
        }
        let mut builder = TrayIconBuilder::new()
            .with_menu(Box::new(build_menu(cx)))
            .with_tooltip(t("tray.tooltip"));
        if let Some(icon) = icon() {
            builder = builder.with_icon_templated(icon);
        }
        match builder.build() {
            Ok(icon) => *tray = Some(icon),
            Err(err) => log::warn!("failed to create the menu bar icon: {err}"),
        }
    });
}

pub fn init(cx: &mut App) {
    let (tx, rx) = async_channel::unbounded::<String>();
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        let _ = tx.try_send(event.id.0.clone());
    }));
    cx.spawn(async move |cx| {
        while let Ok(id) = rx.recv().await {
            cx.update(|cx| match id.as_str() {
                TOGGLE => Dictation::global(cx).update(cx, |d, cx| d.toggle_listening(cx)),
                PANEL => {
                    if control_panel_visible(cx) {
                        crate::close_control_panel(cx);
                    } else {
                        crate::show_control_panel(cx);
                    }
                    sync(cx);
                }
                QUIT => cx.dispatch_action(&crate::Quit),
                _ => {}
            });
        }
    })
    .detach();
    let state = AppState::global(cx);
    cx.observe(&state, |_, cx| sync(cx)).detach();
    cx.subscribe(&state, |_, _: &AppEvent, cx| sync(cx))
        .detach();
    sync(cx);
}
