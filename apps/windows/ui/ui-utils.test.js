import assert from "node:assert/strict";
import test from "node:test";

import {
  escapeHtml,
  shouldShowTimelineLoading,
  getRestoredPeriod,
  timelineStartDate,
  shouldShowAntigravityUltraSetting,
  quotaPresentation,
  subscriptionPresentation,
  visibleTokenBreakdownEntries,
} from "./ui-utils.js";

test("escapes untrusted log labels before inserting them into HTML", () => {
  assert.equal(escapeHtml('<img src=x onerror="alert(1)">'), "&lt;img src=x onerror=&quot;alert(1)&quot;&gt;");
});

test("shows a clean loading state when the selected timeline is not cached", () => {
  assert.equal(shouldShowTimelineLoading({ today: {} }, "week"), true);
  assert.equal(shouldShowTimelineLoading({ today: {} }, "today"), false);
});

test("omits unsupported cache-write metrics instead of displaying zero", () => {
  assert.deepEqual(
    visibleTokenBreakdownEntries({ input: 10, output: 5, cacheRead: 2 }),
    [
      ["Input", 10],
      ["Output", 5],
      ["Cache read", 2],
    ],
  );
});

test("does not invent a paid subscription price when none was detected", () => {
  assert.deepEqual(subscriptionPresentation({ plan: "unknown", pricePerMonth: null }), {
    plan: "Free",
    monthlyPrice: null,
  });
});

test("keeps a detected plan without inventing its unknown monthly price", () => {
  assert.deepEqual(subscriptionPresentation({ plan: "Pro", pricePerMonth: null }), {
    plan: "Pro",
    monthlyPrice: null,
  });
});

test("restores the last valid timeline instead of defaulting to all time", () => {
  assert.equal(getRestoredPeriod("week", ["all", "week"]), "week");
  assert.equal(getRestoredPeriod("not-a-period", ["all", "week"]), "all");
});

test("starts year-to-date charts on January 1 of the current year", () => {
  assert.equal(timelineStartDate("ytd", new Date("2026-09-15T12:00:00Z")), "2026-01-01");
});

test("shows the Antigravity Ultra setting only for a detected Ultra plan", () => {
  assert.equal(shouldShowAntigravityUltraSetting({ plan: "Ultra" }), true);
  assert.equal(shouldShowAntigravityUltraSetting({ plan: "Pro" }), false);
  assert.equal(shouldShowAntigravityUltraSetting(null), false);
});

test("uses the provider reset timestamp for live quota presentation", () => {
  const result = quotaPresentation(
    { usedPercent: 25, resetDate: "2026-09-15T14:30:00Z" },
    new Date("2026-09-15T12:00:00Z"),
  );
  assert.equal(result.remainingPercent, 75);
  assert.equal(result.resetInMinutes, 150);
  assert.equal(result.resetDate.toISOString(), "2026-09-15T14:30:00.000Z");
});
