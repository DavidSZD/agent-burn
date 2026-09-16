import assert from "node:assert/strict";
import test from "node:test";

import { RequestGate } from "./request-gate.js";

test("only the latest period request may update the dashboard", () => {
  const gate = new RequestGate();
  const first = gate.begin();
  const second = gate.begin();

  assert.equal(gate.isCurrent(first), false);
  assert.equal(gate.isCurrent(second), true);
});
