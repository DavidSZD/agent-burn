export function escapeHtml(value) {
  return String(value)
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#39;");
}

export function visibleTokenBreakdownEntries(breakdown) {
  const entries = [
    ["Input", breakdown.input],
    ["Output", breakdown.output],
  ];
  if (breakdown.cacheWrite != null) entries.push(["Cache write", breakdown.cacheWrite]);
  if (breakdown.cacheRead != null) entries.push(["Cache read", breakdown.cacheRead]);
  return entries;
}

export function aggregateTokenBreakdown(agents) {
  const total = { input: 0, output: 0, cacheWrite: 0, cacheRead: 0 };
  for (const agent of agents || []) {
    const breakdown = agent?.tokenBreakdown || {};
    for (const key of Object.keys(total)) total[key] += Number(breakdown[key]) || 0;
  }
  return total;
}

export function visibleAgents(detectedAgents, hiddenAgents) {
  const hidden = new Set(hiddenAgents || []);
  return (detectedAgents || []).filter((agent) => !hidden.has(agent));
}

export function restoredTab(storedTab, detectedAgents) {
  if (storedTab === "summary" || storedTab === "settings") return storedTab;
  return (detectedAgents || []).includes(storedTab) ? storedTab : "summary";
}

export function modelPricingTooltip(pricing) {
  if (!pricing) return "Pricing unavailable";
  const rate = (value) => `$${Number(value).toString()} / 1M`;
  return [
    `Input ${rate(pricing.inputPerM)}`,
    `Cached input ${rate(pricing.cacheReadPerM)}`,
    `Cache write ${rate(pricing.cacheWritePerM)}`,
    `Output ${rate(pricing.outputPerM)}`,
  ].join(" · ");
}

export function subscriptionPresentation(subscription) {
  const rawPlan = String(subscription?.plan || "").trim();
  const unknownPlan = !rawPlan || /^(unknown|plan not detected)$/i.test(rawPlan);
  const explicitlyFree = /^free$/i.test(rawPlan);
  const price = Number(subscription?.pricePerMonth);
  return {
    plan: unknownPlan ? "Free" : rawPlan,
    monthlyPrice: explicitlyFree ? 0 : (Number.isFinite(price) && price > 0 ? price : null),
  };
}

export function timelineSelection(periodCache, period, currentReport, currentAntigravity) {
  const cached = periodCache[period];
  if (cached) return { ...cached, pending: false };
  return {
    reportData: currentReport,
    antigravityData: currentAntigravity,
    pending: true,
  };
}

export function isTimelineCacheFresh(entry, now = Date.now()) {
  return Number.isFinite(entry?.updatedAt) && now - entry.updatedAt <= 5 * 60 * 1000;
}

export function latestTimelineUpdatedAt(periodCache) {
  const timestamps = Object.values(periodCache || {})
    .map((entry) => entry?.updatedAt)
    .filter(Number.isFinite);
  return timestamps.length > 0 ? Math.max(...timestamps) : null;
}

export function waitForInitialRefresh(
  initialRefresh,
  timeoutMs = 125_000,
  schedule = setTimeout,
) {
  return Promise.race([
    initialRefresh,
    new Promise((resolve) => schedule(resolve, timeoutMs)),
  ]);
}

function isSameLocalDay(leftTimestamp, rightTimestamp) {
  const left = new Date(leftTimestamp);
  const right = new Date(rightTimestamp);
  return left.getFullYear() === right.getFullYear()
    && left.getMonth() === right.getMonth()
    && left.getDate() === right.getDate();
}

export function refreshStaticTimelineFreshness(periodCache, updatedAt = Date.now()) {
  const yesterday = periodCache.yesterday;
  if (!Number.isFinite(yesterday?.updatedAt)) return;
  if (isSameLocalDay(yesterday.updatedAt, updatedAt)) {
    yesterday.updatedAt = updatedAt;
  }
}

export function shouldRefreshTimelineInBackground(period, entry, now = Date.now()) {
  if (!entry) return true;
  if (period === "yesterday") {
    return Number.isFinite(entry.updatedAt) && !isSameLocalDay(entry.updatedAt, now);
  }
  return !isTimelineCacheFresh(entry, now);
}

export function timelinePeriodEntries(periodLabels, includeResetToDate = true) {
  return Object.entries(periodLabels).filter(([period]) => includeResetToDate || period !== "rtd");
}

export function loadQuotaHistory(invoke) {
  return invoke("get_quota_history");
}

export function createCoalescedSaver(write) {
  let pending;
  let active = null;

  return (data) => {
    pending = data;
    if (!active) {
      active = (async () => {
        while (pending !== undefined) {
          const next = pending;
          pending = undefined;
          await write(next);
        }
      })().finally(() => {
        active = null;
      });
    }
    return active;
  };
}

export function createSingleFlight(operation) {
  let active = null;
  return (...args) => {
    if (!active) {
      active = Promise.resolve(operation(...args)).finally(() => {
        active = null;
      });
    }
    return active;
  };
}

export function resetWindowStartDate(resetDate, elapsedMinutes, now = new Date()) {
  const reset = resetDate ? new Date(resetDate) : null;
  if (reset && Number.isFinite(reset.getTime())) {
    return new Date(reset.getTime() - 7 * 24 * 60 * 60 * 1000).toISOString().slice(0, 10);
  }
  const elapsed = Number(elapsedMinutes);
  if (!Number.isFinite(elapsed) || elapsed < 0) return null;
  return new Date(now.getTime() - elapsed * 60 * 1000).toISOString().slice(0, 10);
}

function applyNumberDelta(target, fresh, previous, key) {
  const current = Number(target?.[key]) || 0;
  const delta = (Number(fresh?.[key]) || 0) - (Number(previous?.[key]) || 0);
  target[key] = Math.max(0, current + delta);
}

function applyRowsDelta(targetRows, freshRows, previousRows, key) {
  for (const fresh of freshRows || []) {
    const id = fresh?.[key];
    if (id == null) continue;
    let target = targetRows.find((row) => row?.[key] === id);
    if (!target) {
      target = structuredClone(fresh);
      targetRows.push(target);
      continue;
    }
    const previous = (previousRows || []).find((row) => row?.[key] === id) || {};
    for (const field of [
      "totalCost",
      "totalTokens",
      "inputTokens",
      "outputTokens",
      "cacheReadTokens",
      "cacheWriteTokens",
    ]) {
      if (fresh?.[field] != null || previous?.[field] != null || target?.[field] != null) {
        applyNumberDelta(target, fresh, previous, field);
      }
    }
  }
}

export function updateCachedReportsFromToday(periodCache, freshToday, updatedAt = Date.now()) {
  const previousToday = periodCache.today?.reportData;
  periodCache.today = { reportData: structuredClone(freshToday), antigravityData: null, updatedAt };
  refreshStaticTimelineFreshness(periodCache, updatedAt);
  if (!previousToday) return;

  for (const [period, entry] of Object.entries(periodCache)) {
    if (period === "today" || period === "yesterday" || !entry?.reportData) continue;
    const report = entry.reportData;
    report.totals ||= {};
    applyNumberDelta(report.totals, freshToday.totals, previousToday.totals, "totalCost");
    applyNumberDelta(report.totals, freshToday.totals, previousToday.totals, "totalTokens");

    const todayDate = freshToday.daily?.[0]?.date;
    if (todayDate) {
      report.daily = (report.daily || []).filter((day) => day.date !== todayDate);
      report.daily.push(...structuredClone(freshToday.daily || []));
      report.daily.sort((left, right) => String(left.date).localeCompare(String(right.date)));
    }
    report.models ||= [];
    applyRowsDelta(report.models, freshToday.models, previousToday.models, "model");
    report.agents ||= [];
    const existingAgents = new Set(report.agents.map((agent) => agent.agent));
    applyRowsDelta(report.agents, freshToday.agents, previousToday.agents, "agent");
    for (const freshAgent of freshToday.agents || []) {
      const targetAgent = report.agents.find((agent) => agent.agent === freshAgent.agent);
      const previousAgent = (previousToday.agents || []).find((agent) => agent.agent === freshAgent.agent) || {};
      if (!targetAgent) continue;
      if (!existingAgents.has(freshAgent.agent)) continue;
      targetAgent.models ||= [];
      applyRowsDelta(targetAgent.models, freshAgent.models, previousAgent.models, "model");
      if (todayDate) {
        targetAgent.daily = (targetAgent.daily || []).filter((day) => day.date !== todayDate);
        targetAgent.daily.push(...structuredClone(freshAgent.daily || []));
        targetAgent.daily.sort((left, right) => String(left.date).localeCompare(String(right.date)));
      }
      targetAgent.tokenBreakdown ||= {};
      for (const key of ["input", "output", "cacheWrite", "cacheRead"]) {
        if (freshAgent.tokenBreakdown?.[key] != null || previousAgent.tokenBreakdown?.[key] != null) {
          applyNumberDelta(targetAgent.tokenBreakdown, freshAgent.tokenBreakdown, previousAgent.tokenBreakdown, key);
        }
      }
    }
    const modelTokens = report.models.reduce((sum, model) => sum + (Number(model.totalTokens) || 0), 0);
    for (const model of report.models) {
      model.percentage = modelTokens > 0 ? ((Number(model.totalTokens) || 0) / modelTokens) * 100 : 0;
    }
    for (const agent of report.agents) {
      const agentModelTokens = (agent.models || []).reduce(
        (sum, model) => sum + (Number(model.totalTokens) || 0),
        0,
      );
      for (const model of agent.models || []) {
        model.percentage = agentModelTokens > 0
          ? ((Number(model.totalTokens) || 0) / agentModelTokens) * 100
          : 0;
      }
    }
    report.subscription = structuredClone(freshToday.subscription || report.subscription);
    entry.updatedAt = updatedAt;
  }
}

export function updateCacheFromAllSnapshot(periodCache, freshAll, updatedAt = Date.now()) {
  const entry = {
    reportData: structuredClone(freshAll),
    antigravityData: null,
    updatedAt,
  };
  periodCache.all = entry;
  return entry;
}

export function updateCacheFromTimelineSnapshot(periodCache, snapshot, updatedAt = Date.now()) {
  const allReport = structuredClone(snapshot);
  delete allReport.timelineReports;
  const allEntry = updateCacheFromAllSnapshot(periodCache, allReport, updatedAt);
  for (const [period, report] of Object.entries(snapshot?.timelineReports || {})) {
    periodCache[period] = {
      reportData: mergeLiveSubscription(structuredClone(report), allReport),
      antigravityData: null,
      updatedAt,
    };
  }
  return allEntry;
}

export function timelinePreloadOrder(periods, activePeriod) {
  return periods.filter((period) => period !== activePeriod);
}

export function getRestoredPeriod(storedPeriod, availablePeriods) {
  return availablePeriods.includes(storedPeriod) ? storedPeriod : "all";
}

export function timelineStartDate(period, now = new Date()) {
  const year = now.getUTCFullYear();
  if (period === "ytd") return `${year}-01-01`;
  if (period === "wtd") {
    const mondayOffset = (now.getUTCDay() + 6) % 7;
    const monday = new Date(Date.UTC(year, now.getUTCMonth(), now.getUTCDate() - mondayOffset));
    return monday.toISOString().slice(0, 10);
  }
  return null;
}

export function shouldShowAntigravityUltraSetting(plan) {
  return String(plan?.plan || "").trim().toLowerCase() === "ultra";
}

export function quotaPresentation(windowData, now = new Date()) {
  const usedPercent = Math.min(100, Math.max(0, Number(windowData?.usedPercent) || 0));
  const remainingPercent = 100 - usedPercent;
  const resetDate = windowData?.resetDate ? new Date(windowData.resetDate) : null;
  const validResetDate = resetDate && Number.isFinite(resetDate.getTime()) ? resetDate : null;
  const resetInMinutes = validResetDate
    ? Math.max(0, Math.ceil((validResetDate.getTime() - now.getTime()) / 60000))
    : null;
  return { usedPercent, remainingPercent, resetDate: validResetDate, resetInMinutes };
}

export function quotaRemainingPercent(agent) {
  const usedPercent = agent?.window?.usedPercent;
  if (typeof usedPercent === "number" && Number.isFinite(usedPercent)) {
    return Math.min(100, Math.max(0, 100 - usedPercent));
  }

  const liveRemaining = (Array.isArray(agent?.liveLimits) ? agent.liveLimits : [])
    .map((limit) => limit?.remaining)
    .filter((remaining) => typeof remaining === "number" && Number.isFinite(remaining))
    .map((remaining) => Math.min(100, Math.max(0, remaining)));
  return liveRemaining.length > 0 ? Math.min(...liveRemaining) : null;
}

export function mergeLiveSubscription(currentReport, liveReport) {
  if (!currentReport || !liveReport?.subscription) return currentReport;
  return { ...currentReport, subscription: liveReport.subscription };
}

export async function persistQuotaSource(settingsPromise, quotaSource, persist) {
  const settings = await settingsPromise;
  if (!settings) return null;
  return await persist({ ...settings, quotaSource });
}
