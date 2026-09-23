// Launch at login, for ipcHandlers.js and main.js. Decisions live in autoStartPolicy.js.

const { app } = require("electron");
const { resolveAutoStartState, wasLaunchedHidden } = require("./autoStartPolicy");

// { enabled, requiresApproval }
function getAutoStartState() {
  return resolveAutoStartState({ loginItemSettings: app.getLoginItemSettings() });
}

function setAutoStartEnabled(enabled) {
  app.setLoginItemSettings({ openAtLogin: enabled });
}

function wasLaunchedAtLoginHidden() {
  return wasLaunchedHidden({ loginItemSettings: app.getLoginItemSettings() });
}

module.exports = {
  getAutoStartState,
  setAutoStartEnabled,
  wasLaunchedAtLoginHidden,
};
