import assert from "node:assert/strict";
import test from "node:test";

import {
  escapeHtml,
  getRestoredPeriod,
  timelineStartDate,
  shouldShowAntigravityUltraSetting,
  quotaPresentation,
  quotaRemainingPercent,
  mergeLiveSubscription,
  persistQuotaSource,
  timelineSelection,
  timelinePreloadOrder,
  isTimelineCacheFresh,
  subscriptionPresentation,
  visibleTokenBreakdownEntries,
} from "./ui-utils.js";

test("keeps the current dashboard visible while an uncached timeline loads", () => {
  const currentReport = { totals: { totalCost: 12 } };

  assert.deepEqual(timelineSelection({}, "ytd", currentReport, null), {
    reportData: currentReport,
    antigravityData: null,
    pending: true,
  });
});

test("switches immediately when the requested timeline is cached", () => {
  const cached = { reportData: { totals: { totalCost: 42 } }, antigravityData: null, updatedAt: 1_000 };

  assert.deepEqual(timelineSelection({ ytd: cached }, "ytd", { totals: { totalCost: 12 } }, null), {
    ...cached,
    pending: false,
  });
});

test("refreshes a selected timeline only after its five minute freshness window", () => {
  assert.equal(isTimelineCacheFresh({ updatedAt: 1_000 }, 300_999), true);
  assert.equal(isTimelineCacheFresh({ updatedAt: 1_000 }, 301_001), false);
  assert.equal(isTimelineCacheFresh({ reportData: {} }, 2_000), false);
});

test("preloads inactive timelines in their normal display order", () => {
  assert.deepEqual(
    timelinePreloadOrder(["all", "today", "week", "ytd", "month"], "week"),
    ["all", "today", "ytd", "month"],
  );
});

test("waits for settings before persisting a quota source", async () => {
  let resolveSettings;
  const settingsPromise = new Promise((resolve) => {
    resolveSettings = resolve;
  });
  const persisted = [];
  const pending = persistQuotaSource(settingsPromise, "codex", async (settings) => {
    persisted.push(settings);
    return settings;
  });

  assert.deepEqual(persisted, []);
  resolveSettings({ offline: true, refreshMinutes: 5, quotaSource: "antigravity" });

  assert.deepEqual(await pending, { offline: true, refreshMinutes: 5, quotaSource: "codex" });
  assert.deepEqual(persisted, [{ offline: true, refreshMinutes: 5, quotaSource: "codex" }]);
});

test("does not persist a quota source when settings failed to load", async () => {
  let called = false;
  const result = await persistQuotaSource(Promise.resolve(null), "codex", async () => {
    called = true;
  });

  assert.equal(result, null);
  assert.equal(called, false);
});

test("escapes untrusted log labels before inserting them into HTML", () => {
  assert.equal(escapeHtml('<img src=x onerror="alert(1)">'), "&lt;img src=x onerror=&quot;alert(1)&quot;&gt;");
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

test("derives a real remaining percentage from live limits when no window exists", () => {
  assert.equal(
    quotaRemainingPercent({
      liveLimits: [
        { label: "Fast", remaining: 82 },
        { label: "Pro", remaining: 47 },
      ],
    }),
    47,
  );
});

test("does not fabricate a full quota when no usable limit exists", () => {
  assert.equal(
    quotaRemainingPercent({
      window: { usedPercent: null },
      liveLimits: [{ label: "Unknown", remaining: null }],
    }),
    null,
  );
});

test("live quota refresh preserves the selected timeline usage", () => {
  const current = {
    totals: { totalCost: 12 },
    daily: [{ date: "2026-09-16", cost: 12 }],
    subscription: { agents: [{ agent: "codex", window: { usedPercent: 10 } }] },
  };
  const live = {
    totals: { totalCost: 999 },
    daily: [{ date: "2020-01-01", cost: 999 }],
    subscription: { agents: [{ agent: "codex", window: { usedPercent: 25 } }] },
  };

  assert.deepEqual(mergeLiveSubscription(current, live), {
    totals: { totalCost: 12 },
    daily: [{ date: "2026-09-16", cost: 12 }],
    subscription: { agents: [{ agent: "codex", window: { usedPercent: 25 } }] },
  });
});

test("live quota refresh waits for the first timeline report", () => {
  const live = {
    totals: { totalCost: 999 },
    subscription: { agents: [{ agent: "codex", window: { usedPercent: 25 } }] },
  };

  assert.equal(mergeLiveSubscription(null, live), null);
});
