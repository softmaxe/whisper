//! Application entry: wires the services into the GPUI app and its windows.

use std::sync::Arc;

use gpui_kit::*;

use crate::native::hotkeys::{DictationStatus, HotkeyCommand, HotkeyManager};
use crate::settings::{ActivationMode, Settings};

use crate::app_state::{AppEvent, AppState};
use crate::dictation::{Dictation, DictationNotice, StatusMirror};
use crate::ui::control_panel::{CloseOverlay, ControlPanel, OpenSearch, OpenSettings};
use crate::ui::overlays::{Toast, ToastVariant, show_toast};
use crate::ui::pill::{PillView, WINDOW_SIZE};
use crate::ui::settings_modal::SettingsSection;
use crate::{backend, macos, menu, tray, ui};

actions!(whisper_app, [Quit, ShowSettings]);

struct StatusAdapter(StatusMirror);

impl DictationStatus for StatusAdapter {
    fn is_active(&self) -> bool {
        self.0.is_active()
    }

    fn is_processing(&self) -> bool {
        self.0.is_processing()
    }
}

/// The control panel window, kept so the tray and Dock can bring it back,
/// and its view, so other windows can open a Settings section in it.
struct ControlPanelWindow(AnyWindowHandle, WeakEntity<ControlPanel>);
impl Global for ControlPanelWindow {}

fn open_control_panel(cx: &mut App) -> anyhow::Result<ControlPanelWindow> {
    let bounds = Bounds::centered(None, size(px(1200.), px(800.)), cx);
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: Some(TitlebarOptions {
            title: Some("Whisper".into()),
            appears_transparent: true,
            traffic_light_position: Some(point(px(20.), px(20.))),
        }),
        window_min_size: Some(size(px(720.), px(480.))),
        app_id: Some("com.softmaxe.whisper".into()),
        ..Default::default()
    };
    let (handle, panel) = gpui_kit::open_window(options, cx, |window, cx| {
        // Closing the panel drops it to the menu bar.
        window.on_window_should_close(cx, |_, _| {
            crate::native::dock::set_control_panel_visible(false);
            true
        });
        ui::theme::follow_system_appearance(window);
        cx.new(|cx| ControlPanel::new(window, cx))
    })?;
    Ok(ControlPanelWindow(handle, panel.downgrade()))
}

/// Closes the control panel window and hides the Dock icon. Deferred so it
/// also works from a key binding, while the window is still borrowed.
pub fn close_control_panel(cx: &mut App) {
    cx.defer(|cx| {
        for window in cx.windows() {
            if window.downcast::<gpui_kit::base::Root>().is_some() {
                let _ = window.update(cx, |_, window, _| window.remove_window());
            }
        }
        crate::native::dock::set_control_panel_visible(false);
    });
}

/// Shows the control panel and opens Settings, at `section` when given.
pub fn show_settings(section: Option<SettingsSection>, cx: &mut App) {
    show_control_panel(cx);
    let Some(ControlPanelWindow(handle, panel)) = cx.try_global::<ControlPanelWindow>() else {
        return;
    };
    let (handle, panel) = (*handle, panel.clone());
    let _ = handle.update(cx, |_, window, _| {
        // A freshly opened panel has no rendered tree to dispatch into yet.
        window.on_next_frame(move |window, cx| match section {
            Some(section) => {
                let _ = panel.update(cx, |panel, cx| {
                    panel.open_settings(Some(section), window, cx)
                });
            }
            None => window.dispatch_action(Box::new(OpenSettings), cx),
        });
        window.refresh();
    });
}

/// Whether the Settings sheet is open in the control panel. False once the
/// panel window is closed, since its view goes with it.
pub fn settings_open(cx: &App) -> bool {
    cx.try_global::<ControlPanelWindow>()
        .and_then(|w| w.1.upgrade())
        .is_some_and(|panel| panel.read(cx).settings_open())
}

/// Shows the control panel, reopening it if it was closed.
pub fn show_control_panel(cx: &mut App) {
    let existing = cx.try_global::<ControlPanelWindow>().map(|w| w.0);
    if let Some(handle) = existing
        && handle
            .update(cx, |_, window, _| window.activate_window())
            .is_ok()
    {
        cx.activate(true);
        crate::native::dock::set_control_panel_visible(true);
        return;
    }
    match open_control_panel(cx) {
        Ok(window) => {
            cx.set_global(window);
            cx.activate(true);
            crate::native::dock::set_control_panel_visible(true);
        }
        Err(err) => log::error!("failed to open the control panel: {err}"),
    }
}

fn open_pill_window(cx: &mut App) -> anyhow::Result<()> {
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::new(
            point(px(0.), px(0.)),
            WINDOW_SIZE,
        ))),
        titlebar: None,
        focus: false,
        show: false,
        kind: WindowKind::PopUp,
        is_movable: true,
        is_resizable: false,
        is_minimizable: false,
        window_background: WindowBackgroundAppearance::Transparent,
        ..Default::default()
    };
    cx.open_window(options, |window, cx| {
        ui::theme::follow_system_appearance(window);
        cx.new(|cx| PillView::new(window, cx))
    })?;
    Ok(())
}

fn handle_hotkey(command: HotkeyCommand, cx: &mut App) {
    let dictation = Dictation::global(cx);
    match command {
        HotkeyCommand::Toggle => dictation.update(cx, |d, cx| d.toggle(cx)),
        HotkeyCommand::Start => dictation.update(cx, |d, cx| d.start(cx)),
        HotkeyCommand::Stop => dictation.update(cx, |d, cx| d.stop(cx)),
        HotkeyCommand::Prepare => dictation.update(cx, |d, cx| d.prepare(cx)),
        HotkeyCommand::CancelPreparation => dictation.update(cx, |d, cx| d.cancel_preparation(cx)),
        HotkeyCommand::Cancel => dictation.update(cx, |d, cx| d.cancel_recording(cx)),
        HotkeyCommand::CancelKeyPressed => dictation.update(cx, |d, cx| {
            d.cancel_recording(cx);
            cx.emit(DictationNotice::DismissRecovery);
        }),
        HotkeyCommand::ForceStop => dictation.update(cx, |d, _| d.note_force_stopped()),
        HotkeyCommand::UnusedPress { mode } => dictation.update(cx, |_, cx| {
            cx.emit(DictationNotice::HotkeyHint {
                hold: mode == ActivationMode::Push,
            })
        }),
        HotkeyCommand::RegistrationFailed {
            hotkey,
            message_key,
        } => {
            let label = crate::text::hotkeys::format_hotkey_list_label(&hotkey);
            show_toast(
                Toast::new(crate::i18n::t("app.toasts.hotkeyUnavailable.title"))
                    .description(crate::i18n::t_with(&message_key, &[("hotkey", &label)]))
                    .variant(ToastVariant::Destructive),
                cx,
            );
        }
        HotkeyCommand::ListenerError { message } => log::warn!("hotkey listener error: {message}"),
    }
}

fn start_hotkeys(cx: &mut App) {
    let status = Dictation::global(cx).read(cx).status();
    let (tx, rx) = async_channel::unbounded();
    let manager = HotkeyManager::new(Arc::new(StatusAdapter(status)), tx);
    manager.start_native_listener();
    let settings = AppState::settings(cx);
    let registration = manager.register_saved(
        &settings.effective_dictation_key(),
        settings.activation_mode,
    );
    if registration.active != settings.effective_dictation_key()
        || registration.mode != settings.activation_mode
    {
        AppState::global(cx).update(cx, |s, cx| {
            s.update_settings(cx, |s| {
                s.dictation_key = registration.active.clone();
                s.activation_mode = registration.mode;
            })
        });
    }
    backend::install_hotkeys(manager);
    cx.spawn(async move |cx| {
        while let Ok(command) = rx.recv().await {
            cx.update(|cx| handle_hotkey(command, cx));
        }
    })
    .detach();

    // Keep the native activation mode in step with Settings.
    let state = AppState::global(cx);
    let mut last_mode = settings.activation_mode;
    cx.subscribe(&state, move |state, event: &AppEvent, cx| {
        if !matches!(event, AppEvent::SettingsChanged) {
            return;
        }
        let mode = state.read(cx).settings.activation_mode;
        if mode != last_mode {
            last_mode = mode;
            if let Some(manager) = backend::hotkeys()
                && !manager.set_activation_mode(mode)
            {
                state.update(cx, |s, cx| {
                    s.update_settings(cx, |s| s.activation_mode = ActivationMode::Tap)
                });
            }
        }
    })
    .detach();
}

pub fn run() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    // A second process would answer the same shortcut and paste every
    // dictation twice.
    let _instance = match crate::instance::acquire(crate::paths::data_dir()) {
        Ok(crate::instance::Acquired::Locked(lock)) => Some(lock),
        Ok(crate::instance::Acquired::HeldBy(pid)) => {
            log::warn!("Whisper is already running (pid {pid:?}); exiting");
            return;
        }
        Err(err) => {
            log::error!("failed to take the instance lock: {err}");
            None
        }
    };
    let settings = Settings::load();
    let db = match crate::db::Database::open(&crate::paths::database_path()) {
        Ok(db) => Arc::new(db),
        Err(err) => {
            log::error!("failed to open the database: {err}");
            Arc::new(crate::db::Database::open_in_memory().expect("in-memory database"))
        }
    };
    let start_minimized = settings.start_minimized
        || std::env::args().any(|a| a == "--minimized")
        || crate::native::autostart::was_launched_at_login();

    let app = gpui_kit::application().with_assets(ui::assets::Assets);
    // Clicking the Dock icon brings the control panel back.
    app.on_reopen(show_control_panel);
    app.run(move |cx| {
        gpui_kit::init(cx);
        ui::assets::load_fonts(cx);
        macos::track_reduce_motion(cx);
        AppState::init(settings, db, cx);
        ui::overlays::Overlays::init(cx);
        Dictation::init(cx);
        cx.bind_keys([
            KeyBinding::new("cmd-k", OpenSearch, Some("ControlPanel")),
            KeyBinding::new("cmd-,", ShowSettings, None),
            KeyBinding::new("escape", CloseOverlay, Some("ControlPanel")),
            KeyBinding::new("cmd-q", Quit, None),
        ]);
        cx.on_app_quit(|cx| {
            AppState::global(cx).update(cx, |state, _| state.invalidate_learning());
            if let Some(manager) = backend::hotkeys() {
                manager.shutdown();
            }
            crate::services::runtime::drain_storage();
            async {}
        })
        .detach();
        cx.on_action(|_: &Quit, cx| {
            Dictation::global(cx).update(cx, |d, cx| {
                d.cancel_processing(cx);
                d.cancel_recording(cx);
            });
            if let Some(manager) = backend::hotkeys() {
                manager.shutdown();
            }
            cx.quit();
        });
        // Deferred: Cmd+, dispatches while the control panel window is borrowed.
        cx.on_action(|_: &ShowSettings, cx| cx.defer(|cx| show_settings(None, cx)));
        menu::init(cx);
        if let Err(err) = open_pill_window(cx) {
            log::error!("failed to open the recording pill window: {err}");
        }
        // A dev scene is for screenshots; it leaves the shortcut to the real app.
        #[cfg(debug_assertions)]
        let dev_scene = ui::dev_scene::scene().is_some();
        #[cfg(not(debug_assertions))]
        let dev_scene = false;
        if !dev_scene {
            start_hotkeys(cx);
        }
        #[cfg(debug_assertions)]
        {
            let task = cx.spawn(async |cx| {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(800))
                    .await;
                cx.update(ui::dev_scene::apply_pill);
            });
            task.detach();
        }
        cx.background_spawn(async { crate::native::mic::watch_laptop_lid() })
            .detach();
        tray::init(cx);
        crate::native::dock::set_control_panel_visible(!start_minimized);
        if !start_minimized {
            show_control_panel(cx);
        }
        // Retention sweeps run at startup.
        let state = AppState::global(cx).read(cx);
        let (db, storage, settings) = (
            state.db.clone(),
            state.audio.clone(),
            state.settings.clone(),
        );
        let sweep = cx.background_spawn(async move {
            crate::services::runtime::storage(move || {
                crate::audio_storage::run_retention_cleanup(&db, &storage, (&settings).into())
            })
            .await
        });
        cx.spawn(async move |cx| {
            let report = sweep.await;
            log::info!("retention cleanup: {report:?}");
            cx.update(|cx| {
                AppState::global(cx).update(cx, |s, cx| s.apply_retention_cleanup(&report, cx))
            });
        })
        .detach();
    });
}
