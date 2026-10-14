//! The application menu bar.

use gpui_kit::component::input::{Copy, Cut, Paste, Redo, SelectAll, Undo};
use gpui_kit::*;

use whisper::i18n::t;

use crate::app_state::{AppEvent, AppState};
use crate::{Quit, ShowSettings};

const LEARN_MORE_URL: &str = "https://github.com/softmaxe/whisper";

actions!(
    app_menu,
    [
        About,
        HideApp,
        HideOthers,
        ShowAll,
        Minimize,
        CloseWindow,
        BringAllToFront,
        LearnMore
    ]
);

fn menus() -> Vec<Menu> {
    vec![
        Menu::new(t("menu.appLabel")).items([
            MenuItem::action("About Whisper", About),
            MenuItem::separator(),
            MenuItem::action(t("menu.settings"), ShowSettings),
            MenuItem::separator(),
            MenuItem::os_submenu("Services", SystemMenuType::Services),
            MenuItem::separator(),
            MenuItem::action("Hide Whisper", HideApp),
            MenuItem::action("Hide Others", HideOthers),
            MenuItem::action("Show All", ShowAll),
            MenuItem::separator(),
            MenuItem::action(t("menu.quit"), Quit),
        ]),
        Menu::new("Edit").items([
            MenuItem::os_action("Undo", Undo, OsAction::Undo),
            MenuItem::os_action("Redo", Redo, OsAction::Redo),
            MenuItem::separator(),
            MenuItem::os_action("Cut", Cut, OsAction::Cut),
            MenuItem::os_action("Copy", Copy, OsAction::Copy),
            MenuItem::os_action("Paste", Paste, OsAction::Paste),
            MenuItem::os_action("Select All", SelectAll, OsAction::SelectAll),
        ]),
        // AppKit fills View with "Enter Full Screen" by itself.
        Menu::new("View"),
        Menu::new("Window").items([
            MenuItem::action("Minimize", Minimize),
            MenuItem::action("Close", CloseWindow),
            MenuItem::separator(),
            MenuItem::action("Bring All to Front", BringAllToFront),
        ]),
        Menu::new(t("menu.help")).items([MenuItem::action(t("menu.learnMore"), LearnMore)]),
    ]
}

/// Deferred: menu key bindings dispatch while the active window is borrowed.
fn with_active_window(cx: &mut App, f: impl FnOnce(&mut Window, &mut App) + 'static) {
    if let Some(handle) = cx.active_window() {
        cx.defer(move |cx| {
            let _ = handle.update(cx, |_, window, cx| f(window, cx));
        });
    }
}

pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-h", HideApp, None),
        KeyBinding::new("alt-cmd-h", HideOthers, None),
        KeyBinding::new("cmd-m", Minimize, None),
        KeyBinding::new("cmd-w", CloseWindow, None),
    ]);
    cx.on_action(|_: &About, cx| {
        crate::macos::show_about_panel();
        cx.activate(true);
    });
    cx.on_action(|_: &HideApp, cx| cx.hide());
    cx.on_action(|_: &HideOthers, cx| cx.hide_other_apps());
    cx.on_action(|_: &ShowAll, cx| cx.unhide_other_apps());
    cx.on_action(|_: &Minimize, cx| with_active_window(cx, |window, _| window.minimize_window()));
    cx.on_action(|_: &CloseWindow, cx| crate::close_control_panel(cx));
    cx.on_action(|_: &BringAllToFront, cx| cx.activate(true));
    cx.on_action(|_: &LearnMore, cx| cx.open_url(LEARN_MORE_URL));
    cx.set_menus(menus());

    // Menu labels follow the UI language.
    let state = AppState::global(cx);
    let mut language = state.read(cx).settings.ui_language.clone();
    cx.subscribe(&state, move |state, event: &AppEvent, cx| {
        if !matches!(event, AppEvent::SettingsChanged) {
            return;
        }
        let current = &state.read(cx).settings.ui_language;
        if *current != language {
            language = current.clone();
            cx.set_menus(menus());
        }
    })
    .detach();
}
