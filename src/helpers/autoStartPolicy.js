// Launch-at-login and relaunch decisions, kept free of Electron so they can be unit-tested.

function resolveAutoStartState({ loginItemSettings }) {
  return {
    enabled: !!loginItemSettings.openAtLogin,
    // macOS 13+ routes login items through SMAppService, which can register an
    // item and still leave it awaiting approval in System Settings. Unsurfaced,
    // that just looks like a toggle that will not stick.
    requiresApproval: loginItemSettings.status === "requires-approval",
  };
}

// Whether the session started this process rather than the user.
function wasLaunchedHidden({ loginItemSettings }) {
  return !!loginItemSettings.wasOpenedAtLogin;
}

// A relaunch must not replay a cold-start deep link (a sign-in link would
// restore the session a reset just cleared).
function getRelaunchArgs({ argv, protocol }) {
  return argv.slice(1).filter((arg) => !arg.startsWith(`${protocol}://`));
}

module.exports = {
  resolveAutoStartState,
  wasLaunchedHidden,
  getRelaunchArgs,
};
