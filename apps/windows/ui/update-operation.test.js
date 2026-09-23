import assert from "node:assert/strict";
import test from "node:test";

import { createUpdateOperationCoordinator } from "./update-operation.js";

test("queues one footer install request behind an active automatic check", () => {
  const updates = createUpdateOperationCoordinator();

  assert.equal(updates.requestCheck(), "started");
  assert.equal(updates.requestCheck({ installAfterCheck: true }), "queued");
  assert.equal(updates.requestCheck({ installAfterCheck: true }), "queued");
  assert.equal(updates.finishCheck(true), true);
  assert.equal(updates.isCheckRunning(), false);
  assert.equal(updates.isInstallRunning(), true);
  assert.equal(updates.finishCheck(true), false);
  assert.equal(updates.isInstallRunning(), true);
  assert.equal(updates.finishInstall(), undefined);
  assert.equal(updates.isInstallRunning(), false);
});

test("drops a queued install request when a check finds no update", () => {
  const updates = createUpdateOperationCoordinator();

  assert.equal(updates.requestCheck(), "started");
  assert.equal(updates.requestCheck({ installAfterCheck: true }), "queued");
  assert.equal(updates.finishCheck(false), false);
  assert.equal(updates.requestCheck(), "started");
});

test("blocks new checks while an update is being installed", () => {
  const updates = createUpdateOperationCoordinator();

  assert.equal(updates.requestCheck({ installAfterCheck: true }), "started");
  assert.equal(updates.finishCheck(true), true);
  assert.equal(updates.requestCheck(), "installing");
  assert.equal(updates.requestCheck({ installAfterCheck: true }), "installing");
  updates.finishInstall();
  assert.equal(updates.requestCheck(), "started");
});
