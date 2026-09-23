const test = require("node:test");
const assert = require("node:assert/strict");

const helper = require("../../src/helpers/systemDefaultMicrophone");

test("parses the macOS helper's default input", () => {
  assert.deepEqual(
    helper.parseJsonResult('noise\n{"name":" Built-in Microphone ","id":"BuiltIn"}'),
    {
      name: "Built-in Microphone",
      nativeId: "BuiltIn",
    }
  );
  assert.equal(helper.parseJsonResult('{"name":""}'), null);
});

function helperResolver({ now = () => 100, respond } = {}) {
  let calls = 0;
  const resolve = helper.createSystemDefaultMicrophoneResolver({
    now,
    resolveBinary: () => "/helpers/macos-mic-listener",
    run: (command, args) => {
      calls += 1;
      if (command !== "/helpers/macos-mic-listener" || args[0] !== "--print-default-input") {
        throw new Error("unexpected command");
      }
      return respond ? respond() : '{"name":"Desk Microphone"}';
    },
  });
  return { resolve, calls: () => calls };
}

test("resolver keeps a successful lookup until asked to refresh", async () => {
  const { resolve, calls } = helperResolver();

  assert.equal((await resolve()).name, "Desk Microphone");
  assert.equal((await resolve()).name, "Desk Microphone");
  assert.equal(calls(), 1);

  await resolve({ refresh: true });
  assert.equal(calls(), 2);
});

test("resolver retries a failed lookup only after the back-off", async () => {
  let clock = 0;
  let calls = 0;
  const resolve = helper.createSystemDefaultMicrophoneResolver({
    now: () => clock,
    resolveBinary: () => "/helpers/macos-mic-listener",
    run: async () => {
      calls += 1;
      throw new Error("helper failed");
    },
  });

  assert.equal((await resolve()).source, "unavailable");
  clock = 1000;
  await resolve();
  assert.equal(calls, 1, "no retry inside the back-off");
  clock = 31000;
  await resolve();
  assert.equal(calls, 2);
});

test("resolver reports unavailable without the helper binary", async () => {
  const resolve = helper.createSystemDefaultMicrophoneResolver({
    resolveBinary: () => null,
    run: () => {
      throw new Error("must not run");
    },
  });
  assert.deepEqual(await resolve(), { name: "", source: "unavailable" });
});

test("concurrent lookups share one process", async () => {
  let release;
  const { resolve, calls } = helperResolver({
    respond: () =>
      new Promise((done) => {
        release = () => done('{"name":"Desk Microphone"}');
      }),
  });

  const first = resolve();
  const second = resolve({ refresh: true });
  release();

  assert.equal((await first).name, "Desk Microphone");
  assert.equal((await second).name, "Desk Microphone");
  assert.equal(calls(), 1);
});
