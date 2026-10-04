// Launch at login, for ipcHandlers.js and main.js.

const { app } = require("electron");

// { enabled, requiresApproval }
function getAutoStartState() {
  const loginItemSettings = app.getLoginItemSettings();
  return {
    enabled: !!loginItemSettings.openAtLogin,
    // macOS 13+ routes login items through SMAppService, which can register an
    // item and still leave it awaiting approval in System Settings. Unsurfaced,
    // that just looks like a toggle that will not stick.
    requiresApproval: loginItemSettings.status === "requires-approval",
  };
}

function setAutoStartEnabled(enabled) {
  app.setLoginItemSettings({ openAtLogin: enabled });
}

// Whether the session started this process rather than the user.
function wasLaunchedAtLoginHidden() {
  return !!app.getLoginItemSettings().wasOpenedAtLogin;
}

module.exports = {
  getAutoStartState,
  setAutoStartEnabled,
  wasLaunchedAtLoginHidden,
};
