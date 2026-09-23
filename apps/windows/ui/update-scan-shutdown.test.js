import assert from "node:assert/strict";
import test from "node:test";
import { createUpdateScanShutdown } from "./update-scan-shutdown.js";

test("stops scans before running the existing install flow", async () => {
  const shutdown = createUpdateScanShutdown();
  const events = [];
  const result = await shutdown.run({
    onStart: () => events.push("closing"),
    prepare: async () => events.push("stop scans"),
    install: async () => {
      events.push("install flow");
      return true;
    },
    resume: async () => events.push("resume scans"),
  });

  assert.equal(result, true);
  assert.deepEqual(events, ["closing", "stop scans", "install flow"]);
  assert.equal(shutdown.isStopping(), true);
});

test("resumes scans when the user cancels or installation does not hand off", async () => {
  const shutdown = createUpdateScanShutdown();
  let resumes = 0;

  await shutdown.run({
    prepare: async () => {},
    install: async () => false,
    resume: async () => { resumes += 1; },
  });

  assert.equal(resumes, 1);
  assert.equal(shutdown.isStopping(), false);
});

test("resumes scans if update preparation or installation throws", async () => {
  const shutdown = createUpdateScanShutdown();
  let resumes = 0;

  await assert.rejects(shutdown.run({
    prepare: async () => { throw new Error("prepare failed"); },
    install: async () => false,
    resume: async () => { resumes += 1; },
  }), /prepare failed/);

  assert.equal(resumes, 1);
  assert.equal(shutdown.isStopping(), false);
});

test("ignores repeated install clicks while scan shutdown is active", async () => {
  const shutdown = createUpdateScanShutdown();
  let releasePrepare;
  const first = shutdown.run({
    prepare: () => new Promise((resolve) => { releasePrepare = resolve; }),
    install: async () => false,
    resume: async () => {},
  });
  const second = await shutdown.run({
    prepare: async () => assert.fail("duplicate click must not prepare twice"),
    install: async () => assert.fail("duplicate click must not install twice"),
    resume: async () => {},
  });

  assert.equal(second, false);
  releasePrepare();
  await first;
});

test("clears a timeline loading indicator when an update cancels the load", async () => {
  const shutdown = createUpdateScanShutdown();
  let releasePrepare;
  const first = shutdown.run({
    prepare: () => new Promise((resolve) => { releasePrepare = resolve; }),
    install: async () => false,
    resume: async () => {},
  });
  let refreshState = null;

  assert.equal(
    shutdown.clearTimelineRefreshForUpdate((active, source) => { refreshState = { active, source }; }),
    true,
  );
  assert.deepEqual(refreshState, { active: false, source: "timeline" });
  releasePrepare();
  await first;
  assert.equal(shutdown.clearTimelineRefreshForUpdate(() => assert.fail("no update in progress")), false);
});
