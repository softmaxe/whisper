const test = require("node:test");
const assert = require("node:assert/strict");
const Module = require("node:module");

// Stands in for the process's macOS activation policy: "regular" shows the
// Dock icon, "accessory" hides it.
const fakeApp = {
  policy: "regular",
  calls: [],
  setActivationPolicy(policy) {
    this.calls.push(policy);
    this.policy = policy;
  },
  dock: {
    isVisible: () => fakeApp.policy === "regular",
    show() {
      fakeApp.calls.push("dock.show");
      fakeApp.policy = "regular";
    },
    hide() {
      fakeApp.calls.push("dock.hide");
      fakeApp.policy = "accessory";
    },
  },
};

const originalLoad = Module._load;
Module._load = function (request, parent, isMain) {
  if (request === "electron") return { app: fakeApp };
  return originalLoad.call(this, request, parent, isMain);
};
const dockManager = require("../../src/helpers/dockManager");
Module._load = originalLoad;

function setup(t, { policy = "regular" } = {}) {
  const originalPlatform = Object.getOwnPropertyDescriptor(process, "platform");
  Object.defineProperty(process, "platform", { value: "darwin" });
  t.after(() => Object.defineProperty(process, "platform", originalPlatform));
  t.mock.timers.enable({ apis: ["Date"] });
  fakeApp.policy = policy;
  fakeApp.calls.length = 0;
}

test("a launch that opens the control panel keeps the Dock icon it started with", (t) => {
  setup(t);

  dockManager.init({ controlPanelVisible: true });
  dockManager.setControlPanelVisible(true);

  // Dropping to accessory at launch deactivates the app, so the panel that
  // opens a moment later would not get focus.
  assert.deepEqual(fakeApp.calls, []);
  assert.equal(fakeApp.policy, "regular");
});

test("a tray-only launch hides the Dock icon", (t) => {
  setup(t);

  dockManager.init();

  assert.deepEqual(fakeApp.calls, ["accessory"]);
});

test("opening the control panel from the tray shows the icon without app.dock.show()", (t) => {
  setup(t, { policy: "accessory" });
  dockManager.init();

  dockManager.setControlPanelVisible(true);

  // app.dock.show() activates the Dock while the app is active, and macOS
  // refuses to give focus back to the panel the user just opened.
  assert.deepEqual(fakeApp.calls, ["regular"]);
});

test("reopening a visible control panel leaves the activation policy alone", (t) => {
  setup(t);
  dockManager.init({ controlPanelVisible: true });

  dockManager.setControlPanelVisible(true);
  dockManager.setControlPanelVisible(true);

  assert.deepEqual(fakeApp.calls, []);
});

test("closing the control panel right after showing the icon keeps it until the next hide", (t) => {
  setup(t, { policy: "accessory" });
  dockManager.init();
  dockManager.setControlPanelVisible(true);
  fakeApp.calls.length = 0;

  dockManager.setControlPanelVisible(false);
  assert.deepEqual(fakeApp.calls, []);

  t.mock.timers.tick(1000);
  dockManager.setControlPanelVisible(false);
  assert.deepEqual(fakeApp.calls, ["accessory"]);
});
