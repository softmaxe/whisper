const { app } = require("electron");

// Keeps the throttle of Electron's DockHide (browser_mac.mm): hiding the icon
// within this long of showing it can leave duplicate Dock icons behind.
const HIDE_AFTER_SHOW_GUARD_MS = 1000;

// Single owner of the macOS Dock icon. Every caller that wants the icon shown
// or hidden goes through here, and the icon simply tracks the control panel.
//
// Callers report that state explicitly. The window's own "show"/"hide" events
// look like the obvious source to derive it from, but on macOS they are
// occlusion events: Electron only emits them from
// windowDidChangeOcclusionState, so they also fire when the panel is merely
// covered by another window, minimized, or on another Space. Deriving from
// them makes the icon flicker as the user switches windows.
class DockManager {
  constructor() {
    this._controlPanelVisible = false;
    this._lastShownAt = null;
  }

  // Called once at startup, before any window exists. A launch that opens the
  // control panel keeps the icon macOS already shows: dropping to accessory and
  // back would deactivate the app and hand focus to the previous app. Tray-only
  // launches hide the icon until the control panel opens.
  init({ controlPanelVisible = false } = {}) {
    this._controlPanelVisible = !!controlPanelVisible;
    this._lastShownAt = null;
    this._applyVisibility();
  }

  // Reported by every path that surfaces or hides the control panel.
  setControlPanelVisible(visible) {
    this._controlPanelVisible = !!visible;
    this._applyVisibility();
  }

  _applyVisibility() {
    // Hiding the dictation panel must never touch the Dock. The panel is
    // hidden outright rather than minimized into the Dock, so there is nothing
    // to restore from there.
    const visible = this._controlPanelVisible;
    if (!app.dock) return;

    if (visible) {
      if (app.dock.isVisible()) return;
      // Not app.dock.show(): while the app is active, it activates the Dock and
      // reactivates the app a second later, which macOS cooperative activation
      // refuses. The window the user just opened loses focus to the previous
      // app, so launching from Raycast or the Dock left Finder in front.
      // app.dock.hide() is avoided for symmetry: it also marks every window as
      // unhideable, which only app.dock.show() undoes.
      app.setActivationPolicy("regular");
      this._lastShownAt = Date.now();
    } else {
      if (!app.dock.isVisible()) return;
      // Closing the control panel right after opening it leaves the icon up
      // until the next hide. Working around the guard risks the duplicate
      // Dock icons it exists to prevent.
      if (this._lastShownAt !== null && Date.now() - this._lastShownAt < HIDE_AFTER_SHOW_GUARD_MS) {
        return;
      }
      app.setActivationPolicy("accessory");
    }
  }
}

module.exports = new DockManager();
