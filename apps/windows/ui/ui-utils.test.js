import assert from "node:assert/strict";
import test from "node:test";

import {
  createCoalescedSaver,
  createSingleFlight,
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
  loadQuotaHistory,
  resetWindowStartDate,
  refreshStaticTimelineFreshness,
  shouldRefreshTimelineInBackground,
  timelinePeriodEntries,
  updateCachedReportsFromToday,
  subscriptionPresentation,
  visibleTokenBreakdownEntries,
} from "./ui-utils.js";

test("coalesced saves keep the in-flight write and only the newest pending state", async () => {
  let releaseFirst;
  const writes = [];
  const save = createCoalescedSaver(async (data) => {
    writes.push(data.revision);
    if (data.revision === 1) await new Promise((resolve) => { releaseFirst = resolve; });
  });

  const first = save({ revision: 1 });
  save({ revision: 2 });
  const latest = save({ revision: 3 });
  releaseFirst();
  await Promise.all([first, latest]);

  assert.deepEqual(writes, [1, 3]);
});

test("single-flight refreshes share one active operation", async () => {
  let release;
  let calls = 0;
  const run = createSingleFlight(async () => {
    calls += 1;
    await new Promise((resolve) => { release = resolve; });
    return "done";
  });

  const first = run();
  const second = run();
  release();

  assert.equal(await first, "done");
  assert.equal(await second, "done");
  assert.equal(calls, 1);
});

test("general timeline choices omit reset to date", () => {
  assert.deepEqual(
    timelinePeriodEntries({ all: "All time", rtd: "Reset to date", ytd: "Year to date" }, false),
    [["all", "All time"], ["ytd", "Year to date"]],
  );
});

test("quota history is loaded independently from a timeline refresh", async () => {
  const calls = [];
  const history = [{ timestamp: "2026-09-16T10:00:00Z", agents: [] }];

  const result = await loadQuotaHistory(async (command) => {
    calls.push(command);
    return history;
  });

  assert.deepEqual(calls, ["get_quota_history"]);
  assert.deepEqual(result, history);
});

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

test("derives reset-to-date start from a weekly provider reset", () => {
  assert.equal(resetWindowStartDate("2026-09-23T04:34:22Z"), "2026-09-16");
  assert.equal(resetWindowStartDate(null, 48 * 60, new Date("2026-09-16T12:00:00Z")), "2026-09-14");
  assert.equal(resetWindowStartDate(null), null);
});

test("a fresh today report advances every timeline that contains today", () => {
  const cache = {
    today: { reportData: { totals: { totalCost: 2, totalTokens: 20 }, daily: [{ date: "2026-09-16", cost: 2, tokens: 20 }], agents: [], models: [] }, updatedAt: 1 },
    ytd: { reportData: { totals: { totalCost: 12, totalTokens: 120 }, daily: [{ date: "2026-09-16", cost: 2, tokens: 20 }], agents: [], models: [] }, updatedAt: 1 },
    yesterday: { reportData: { totals: { totalCost: 5, totalTokens: 50 }, daily: [], agents: [], models: [] }, updatedAt: 1 },
  };
  const freshToday = { totals: { totalCost: 3, totalTokens: 30 }, daily: [{ date: "2026-09-16", cost: 3, tokens: 30 }], agents: [], models: [], subscription: { agents: [] } };

  updateCachedReportsFromToday(cache, freshToday, 500);

  assert.deepEqual(cache.ytd.reportData.totals, { totalCost: 13, totalTokens: 130 });
  assert.deepEqual(cache.ytd.reportData.daily, [{ date: "2026-09-16", cost: 3, tokens: 30 }]);
  assert.equal(cache.ytd.updatedAt, 500);
  assert.deepEqual(cache.yesterday.reportData.totals, { totalCost: 5, totalTokens: 50 });
  assert.equal(cache.yesterday.updatedAt, 500);
});

test("keeps yesterday fresh without rescanning during the same local day", () => {
  const cache = {
    yesterday: {
      reportData: { totals: { totalCost: 5 } },
      updatedAt: new Date(2026, 8, 16, 8, 0).getTime(),
    },
  };
  const refreshedAt = new Date(2026, 8, 16, 23, 30).getTime();

  refreshStaticTimelineFreshness(cache, refreshedAt);

  assert.equal(cache.yesterday.updatedAt, refreshedAt);
});

test("leaves yesterday stale after local midnight so it can be rescanned", () => {
  const originalUpdatedAt = new Date(2026, 8, 16, 23, 59).getTime();
  const cache = {
    yesterday: {
      reportData: { totals: { totalCost: 5 } },
      updatedAt: originalUpdatedAt,
    },
  };

  refreshStaticTimelineFreshness(cache, new Date(2026, 8, 17, 0, 1).getTime());

  assert.equal(cache.yesterday.updatedAt, originalUpdatedAt);
});

test("refreshes yesterday in the background only after local midnight", () => {
  const yesterdayEntry = { updatedAt: new Date(2026, 8, 16, 23, 59).getTime() };

  assert.equal(
    shouldRefreshTimelineInBackground("yesterday", yesterdayEntry, new Date(2026, 8, 16, 23, 59, 30).getTime()),
    false,
  );
  assert.equal(
    shouldRefreshTimelineInBackground("yesterday", yesterdayEntry, new Date(2026, 8, 17, 0, 1).getTime()),
    true,
  );
  assert.equal(shouldRefreshTimelineInBackground("month", yesterdayEntry), false);
});

test("a newly detected agent is copied once into older timeline caches", () => {
  const cache = {
    today: { reportData: { totals: {}, daily: [], agents: [], models: [] }, updatedAt: 1 },
    ytd: { reportData: { totals: {}, daily: [], agents: [], models: [] }, updatedAt: 1 },
  };
  const freshToday = {
    totals: { totalCost: 2, totalTokens: 20 },
    daily: [{ date: "2026-09-16", cost: 2, tokens: 20 }],
    models: [{ model: "new-model", totalCost: 2, totalTokens: 20, percentage: 100 }],
    agents: [{
      agent: "new-agent",
      totalCost: 2,
      totalTokens: 20,
      models: [{ model: "new-model", totalCost: 2, totalTokens: 20, percentage: 100 }],
      daily: [{ date: "2026-09-16", cost: 2, tokens: 20 }],
      tokenBreakdown: { input: 20 },
    }],
  };

  updateCachedReportsFromToday(cache, freshToday, 500);

  assert.equal(cache.ytd.reportData.agents[0].totalTokens, 20);
  assert.equal(cache.ytd.reportData.agents[0].models[0].totalTokens, 20);
  assert.equal(cache.ytd.reportData.agents[0].tokenBreakdown.input, 20);
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
