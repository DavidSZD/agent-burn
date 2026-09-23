import assert from "node:assert/strict";
import test from "node:test";

import {
  clearPersistedAvailableUpdate,
  createSafeStorage,
  getPersistedAvailableUpdate,
  rememberAvailableUpdate,
} from "./update-notice.js";

function createStorage() {
  const values = new Map();
  return {
    getItem(key) {
      return values.get(key) ?? null;
    },
    setItem(key, value) {
      values.set(key, String(value));
    },
    removeItem(key) {
      values.delete(key);
    },
  };
}

test("restores the available version after an app restart without announcing it again", () => {
  const storage = createStorage();
  const firstDiscovery = rememberAvailableUpdate(storage, "0.1.64");

  assert.equal(firstDiscovery.shouldAnnounce, true);

  // A new app instance sees the same persistent storage after restart.
  assert.equal(getPersistedAvailableUpdate(storage), "0.1.64");
  assert.equal(rememberAvailableUpdate(storage, "0.1.64").shouldAnnounce, false);
});

test("announces a newly detected version once, then keeps its badge state", () => {
  const storage = createStorage();

  assert.equal(rememberAvailableUpdate(storage, "0.1.64").shouldAnnounce, true);
  assert.equal(rememberAvailableUpdate(storage, "0.1.65").shouldAnnounce, true);
  assert.equal(rememberAvailableUpdate(storage, "0.1.65").shouldAnnounce, false);
  assert.equal(getPersistedAvailableUpdate(storage), "0.1.65");
});

test("clears a stale available-version badge when a check finds no update", () => {
  const storage = createStorage();
  rememberAvailableUpdate(storage, "0.1.64");

  clearPersistedAvailableUpdate(storage);

  assert.equal(getPersistedAvailableUpdate(storage), null);
});

test("ignores empty or malformed persisted version values", () => {
  const storage = createStorage();
  storage.setItem("agent-burn-available-update-version", "   ");

  assert.equal(getPersistedAvailableUpdate(storage), null);
  assert.equal(rememberAvailableUpdate(storage, " ").shouldAnnounce, false);
  assert.equal(rememberAvailableUpdate(storage, "next-version").shouldAnnounce, false);
});

test("keeps update discovery usable when browser storage is unavailable", () => {
  const storage = createSafeStorage(() => {
    throw new Error("Storage access denied");
  });

  assert.equal(getPersistedAvailableUpdate(storage), null);
  assert.deepEqual(rememberAvailableUpdate(storage, "0.2.1"), {
    version: "0.2.1",
    shouldAnnounce: true,
  });
  assert.equal(getPersistedAvailableUpdate(storage), "0.2.1");
  assert.equal(rememberAvailableUpdate(storage, "0.2.1").shouldAnnounce, false);
  assert.doesNotThrow(() => clearPersistedAvailableUpdate(storage));
});

test("does not fail update discovery when individual storage operations throw", () => {
  const storage = {
    getItem() {
      throw new Error("Read denied");
    },
    setItem() {
      throw new Error("Write denied");
    },
    removeItem() {
      throw new Error("Remove denied");
    },
  };

  assert.deepEqual(rememberAvailableUpdate(storage, "0.2.2"), {
    version: "0.2.2",
    shouldAnnounce: true,
  });
  assert.equal(rememberAvailableUpdate(storage, "0.2.2").shouldAnnounce, false);
  assert.doesNotThrow(() => clearPersistedAvailableUpdate(storage));
});
