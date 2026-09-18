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
  return modelPricingRows(pricing)
    .map(([label, value]) => `${label} ${value} / 1M`)
    .join(" · ");
}

export function modelPricingRows(pricing) {
  if (!pricing) return [];
  const rate = (value) => `$${Number(value).toString()}`;
  return [
    ["Input", rate(pricing.inputPerM)],
    ["Cached input", rate(pricing.cacheReadPerM)],
    ["Cache write", rate(pricing.cacheWritePerM)],
    ["Output", rate(pricing.outputPerM)],
  ];
}

export function createReplaceableCallback(callback) {
  let current = callback;
  return {
    replace(next) {
      current = next;
    },
    run(...args) {
      return current(...args);
    },
  };
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

function formatElapsedSeconds(seconds) {
  if (seconds < 60) return `${seconds} sec`;
  const minutes = Math.floor(seconds / 60);
  const remainingSeconds = seconds % 60;
  return remainingSeconds > 0 ? `${minutes} min ${remainingSeconds} sec` : `${minutes} min`;
}

export function refreshStatusText({ updatedAt, refreshStartedAt, nextRefreshAt } = {}, now = Date.now()) {
  if (Number.isFinite(refreshStartedAt)) {
    const elapsedSeconds = Math.max(0, Math.floor((now - refreshStartedAt) / 1000));
    return `Updating · ${formatElapsedSeconds(elapsedSeconds)}`;
  }
  if (Number.isFinite(nextRefreshAt)) {
    const untilNext = Math.max(0, Math.ceil((nextRefreshAt - now) / 1000));
    return `Next refresh in ${formatElapsedSeconds(untilNext)}`;
  }
  if (!Number.isFinite(updatedAt)) return "Waiting for first update";

  const elapsedSeconds = Math.max(0, Math.floor((now - updatedAt) / 1000));
  if (elapsedSeconds < 5) return "Updated just now";
  if (elapsedSeconds < 60) return `Updated ${elapsedSeconds} sec ago`;
  return `Updated ${Math.floor(elapsedSeconds / 60)} min ago`;
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

// Replace only the provider rows present in a partial refresh. This keeps a
// slow provider's last valid values visible while a faster provider publishes
// its new report, without double-counting shared model names.
export function mergeSourceSnapshot(periodCache, sourceSnapshot, updatedAt = Date.now()) {
  const current = periodCache.all?.reportData || {
    totals: { totalCost: 0, totalTokens: 0 },
    agents: [],
    models: [],
  };
  const merged = mergeSourceReports(current, sourceSnapshot);
  periodCache.all = {
    reportData: merged,
    antigravityData: periodCache.all?.antigravityData || null,
    updatedAt,
  };
  for (const [period, timelineReport] of Object.entries(merged.timelineReports || {})) {
    periodCache[period] = {
      reportData: mergeLiveSubscription(structuredClone(timelineReport), merged),
      antigravityData: periodCache[period]?.antigravityData || null,
      updatedAt,
    };
  }
  return merged;
}

function mergeSourceReports(currentReport, sourceReport) {
  const current = structuredClone(currentReport || {});
  const incomingAgents = Array.isArray(sourceReport?.agents) ? sourceReport.agents : [];
  const sourceNames = new Set(incomingAgents.map((agent) => agent?.agent).filter(Boolean));
  current.agents = (Array.isArray(current.agents) ? current.agents : [])
    .filter((agent) => !sourceNames.has(agent?.agent));
  current.agents.push(...structuredClone(incomingAgents));
  recomputeDailyFromAgents(current);

  const incomingModels = Array.isArray(sourceReport?.models) ? sourceReport.models : [];
  if (incomingModels.length > 0) {
    const sourceModelNames = new Set(incomingModels.map((model) => model?.model).filter(Boolean));
    current.models = (Array.isArray(current.models) ? current.models : [])
      .filter((model) => !sourceModelNames.has(model?.model));
    current.models.push(...structuredClone(incomingModels));
    const totalCost = current.models.reduce((sum, model) => sum + (Number(model.totalCost) || 0), 0);
    for (const model of current.models) {
      model.percentage = totalCost > 0 ? ((Number(model.totalCost) || 0) / totalCost) * 100 : 0;
    }
  }

  // The backend includes the same per-period matrix used to warm every
  // timeline. Merge it recursively so partial provider refreshes update each
  // cached period while preserving slower providers' last valid rows.
  if (sourceReport?.timelineReports && typeof sourceReport.timelineReports === "object") {
    current.timelineReports = current.timelineReports && typeof current.timelineReports === "object"
      ? current.timelineReports
      : {};
    for (const [period, sourceTimeline] of Object.entries(sourceReport.timelineReports)) {
      current.timelineReports[period] = mergeSourceReports(
        current.timelineReports[period] || { totals: { totalCost: 0, totalTokens: 0 }, agents: [], models: [] },
        sourceTimeline,
      );
    }
  }

  const totalCost = current.agents.reduce((sum, agent) => sum + (Number(agent.totalCost) || 0), 0);
  const totalTokens = current.agents.reduce((sum, agent) => sum + (Number(agent.totalTokens) || 0), 0);
  current.totals = { totalCost, totalTokens };

  if (sourceReport?.subscription) {
    const existingSubscription = current.subscription || {};
    const incomingSubscriptions = Array.isArray(sourceReport.subscription.agents)
      ? sourceReport.subscription.agents
      : [];
    const names = new Set(incomingSubscriptions.map((agent) => agent?.agent).filter(Boolean));
    const existingSubscriptions = Array.isArray(existingSubscription.agents)
      ? existingSubscription.agents.filter((agent) => !names.has(agent?.agent))
      : [];
    current.subscription = {
      ...existingSubscription,
      ...structuredClone(sourceReport.subscription),
      agents: [...existingSubscriptions, ...structuredClone(incomingSubscriptions)],
    };
  }
  return current;
}

function recomputeDailyFromAgents(report) {
  const byDate = new Map();
  for (const agent of report.agents || []) {
    for (const day of agent.daily || []) {
      if (!day?.date) continue;
      const current = byDate.get(day.date) || { date: day.date, cost: 0, tokens: 0 };
      current.cost += Number(day.cost) || 0;
      current.tokens += Number(day.tokens) || 0;
      byDate.set(day.date, current);
    }
  }
  if (byDate.size > 0) report.daily = [...byDate.values()].sort((left, right) => left.date.localeCompare(right.date));
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

export function selectedTimelineReport(periodCache, cacheKey, fallbackReport, liveReport) {
  const cachedReport = periodCache?.[cacheKey]?.reportData;
  const report = cachedReport || fallbackReport;
  if (!report) return report;
  const selected = Array.isArray(report.daily) && report.daily.length > 0
    ? report
    : structuredClone(report);
  if (!Array.isArray(selected.daily) || selected.daily.length === 0) {
    recomputeDailyFromAgents(selected);
  }
  return mergeLiveSubscription(selected, liveReport);
}

export async function persistQuotaSource(settingsPromise, quotaSource, persist) {
  const settings = await settingsPromise;
  if (!settings) return null;
  return await persist({ ...settings, quotaSource });
}
