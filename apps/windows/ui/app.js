import { RequestGate } from "./request-gate.js";
import {
  aggregateTokenBreakdown,
  createCoalescedSaver,
  createReplaceableCallback,
  createSingleFlight,
  escapeHtml,
  getRestoredPeriod,
  isTimelineCacheFresh,
  latestTimelineUpdatedAt,
  modelPricingTooltip,
  modelPricingRows,
  loadQuotaHistory,
  resetWindowStartDate,
  restoredTab,
  shouldRefreshTimelineInBackground,
  timelineSelection,
  timelinePreloadOrder,
  timelinePeriodEntries,
  timelineStartDate,
  shouldShowAntigravityUltraSetting,
  quotaPresentation,
  quotaRemainingPercent,
  mergeLiveSubscription,
  persistQuotaSource,
  subscriptionPresentation,
  visibleTokenBreakdownEntries,
  visibleAgents,
  updateCachedReportsFromToday,
  updateCacheFromTimelineSnapshot,
  waitForInitialRefresh,
} from "./ui-utils.js";

// Agent Burn Windows - Client Web / Tauri v2
// Interface fidèle à 100% à la version originale macOS

function getInvoke() {
  if (window.__TAURI__?.core?.invoke) {
    return window.__TAURI__.core.invoke;
  }
  if (window.__TAURI_INTERNALS__?.invoke) {
    return window.__TAURI_INTERNALS__.invoke;
  }
  return null;
}

async function invokeTauri(cmd, args = {}) {
  const inv = getInvoke();
  if (!inv) {
    throw new Error("Liaison Tauri v2 non initialisée. Veuillez relancer l'application.");
  }
  return await inv(cmd, args);
}

function showStatusError(message) {
  const footer = document.getElementById("footer-status-text");
  if (footer) footer.textContent = message;
}

function getListen() {
  return window.__TAURI__?.event?.listen || null;
}

// État de l'application
let currentPeriod = "all"; // "All time" par défaut comme sur les captures macOS
let currentTab = "summary";
let reportData = null;
let fullReportData = null;
let antigravityData = null;
let quotaHistoryData = null;
let latestLiveQuotaReport = null;
let currentSpendGranularity = "monthly"; // "Monthly" actif par défaut sur les captures
let detectedAgents = [];
const allKnownAgents = new Set();
let lastUpdatedTime = 0;
let lastBackendRefreshMs = 0;
let lastBackendRevisionMs = 0;
let appSettings = null;
let settingsLoadPromise = Promise.resolve(null);
const periodCache = {};
const periodRequests = new Map();
const saveReportCache = createCoalescedSaver((data) => invokeTauri("save_report_cache", { data }));
const refreshAllTimelineCaches = createSingleFlight(performAllTimelineRefresh);
const harnessCache = {};
const summaryRequestGate = new RequestGate();
const activeRefreshSources = new Set();
let resolveInitialBackendRefresh;
const initialBackendRefresh = new Promise((resolve) => {
  resolveInitialBackendRefresh = resolve;
});

// Libellés des périodes (alignés avec UsagePeriod de macOS)
const PERIOD_LABELS = {
  all: "All time",
  today: "Today",
  yesterday: "Yesterday",
  rtd: "Reset to date",
  wtd: "Week to date",
  mtd: "This month",
  week: "Last 7 days",
  month: "Last 30 days",
  ytd: "Year to date",
};

// Formattage des nombres façon macOS (compact : 46.01B, 15.42M, 994.0K)
function formatCompactTokens(num) {
  if (!num || num === 0) return "0";
  const n = Number(num);
  if (n >= 1e9) return (n / 1e9).toFixed(2) + "B";
  if (n >= 1e6) return (n / 1e6).toFixed(2) + "M";
  if (n >= 1e3) return (n / 1e3).toFixed(1) + "K";
  return n.toLocaleString();
}

function formatCurrency(num) {
  const n = Number(num) || 0;
  return "$" + n.toLocaleString("en-US", { minimumFractionDigits: 2, maximumFractionDigits: 2 });
}

function formatTokensPerDollar(tokens, cost) {
  if (!cost || cost <= 0 || !tokens || tokens <= 0) return "—";
  const tpd = tokens / cost;
  if (tpd >= 1e6) return (tpd / 1e6).toFixed(1) + "M / $";
  if (tpd >= 1e3) return (tpd / 1e3).toFixed(1) + "K / $";
  return Math.round(tpd) + " / $";
}

function getAgentColor(agent) {
  switch ((agent || "").toLowerCase()) {
    case "codex":
      return "#22c55e"; // Vert vif OpenAI Codex
    case "claude":
      return "#f97316"; // Orange terracotta Claude
    case "cursor":
      return "#a855f7"; // Violet Cursor
    case "antigravity":
      return "#06b6d4"; // Cyan Google Antigravity
    case "hermes":
      return "#ec4899"; // Rose vif Nous Research Hermes
    case "opencode":
      return "#3b82f6";
    default:
      return "#38bdf8";
  }
}

function getAgentBrandIcon(agent) {
  const a = (agent || "").toLowerCase();
  switch (a) {
    case "codex":
      return "brands/codex.png";
    case "claude":
      return "brands/claude.png";
    case "cursor":
      return "brands/cursor.png";
    case "antigravity":
      return "brands/antigravity.png";
    case "gemini":
      return "brands/gemini.png";
    case "hermes":
      return "brands/hermes.png";
    case "opencode":
      return "brands/opencode.png";
    case "openclaw":
      return "brands/openclaw.png";
    case "pi":
      return "brands/pi.png";
    case "kimi":
      return "brands/kimi.png";
    case "qwen":
      return "brands/qwen.png";
    case "amp":
      return "brands/amp.png";
    default:
      return "brands/gemini.png";
  }
}

function getAgentDisplayName(agent) {
  const a = (agent || "").toLowerCase();
  switch (a) {
    case "codex":
      return "Codex";
    case "claude":
      return "Claude Code";
    case "cursor":
      return "Cursor";
    case "antigravity":
      return "Antigravity";
    case "gemini":
      return "Gemini";
    case "hermes":
      return "Hermes";
    case "opencode":
      return "OpenCode";
    case "openclaw":
      return "OpenClaw";
    case "pi":
      return "Pi";
    default:
      return agent.charAt(0).toUpperCase() + agent.slice(1);
  }
}

// ==========================================================================
// Initialisation au chargement
// ==========================================================================
window.addEventListener("DOMContentLoaded", async () => {
  const savedPeriod = localStorage.getItem("agent-burn-period");
  currentPeriod = getRestoredPeriod(savedPeriod, Object.keys(PERIOD_LABELS));
  if (currentPeriod === "rtd") currentPeriod = "all";
  initPeriods();
  initRefresh();
  initSettings();
  setTimelineRefreshing(true, "startup");
  initFooterTimer();
  initBackendEvents();
  initBackendRefreshPolling();

  // Démarrage rapide avec le cache
  await settingsLoadPromise;
  await initColdStart();
  updateTimelineAvailability();
  // On ne bloque le premier affichage que s'il n'existe encore aucune donnée.
  if (!periodCache[currentPeriod]) await loadData(true);
  currentTab = restoredTab(
    localStorage.getItem("agent-burn-tab"),
    visibleAgents(detectedAgents, appSettings?.hiddenAgents),
  );
  switchTab(currentTab);
  // Let live quotas update first, then refresh every stale timeline in the background.
  void refreshStaleTimelinesAfterInitialBackend();
});

function initBackendEvents() {
  const listen = getListen();
  if (!listen) return;
  listen("refresh_requested", async () => {
    await loadData(true);
  });
  listen("quotas_updated", async (event) => {
    if (event?.payload) {
      await syncBackendRefresh(event.payload, Date.now());
    }
  });
}

function applyBackendRefresh(data, refreshedAtMs) {
  if (!data || refreshedAtMs <= lastBackendRefreshMs) return false;
  lastBackendRefreshMs = refreshedAtMs;
  lastUpdatedTime = refreshedAtMs;
  latestLiveQuotaReport = data;
  updateTopBarQuotaPill(data);

  const entry = updateCacheFromTimelineSnapshot(periodCache, data, refreshedAtMs);
  if (currentPeriod === "all") {
    reportData = entry.reportData;
    fullReportData = entry.reportData;
  } else {
    reportData = mergeLiveSubscription(reportData, data);
  }
  computeDetectedAgents(data, antigravityData);
  return true;
}

async function syncBackendRefresh(data, refreshedAtMs) {
  if (!applyBackendRefresh(data, refreshedAtMs)) return;
  resolveInitialBackendRefresh();
  try {
    quotaHistoryData = await invokeTauri("get_quota_history");
  } catch (_) {}
  renderHarnessTabs();
  if (currentTab === "summary") renderSummary();
  else if (currentTab !== "settings") renderHarnessView(currentTab);
  saveReportCache({ summary: reportData, periods: periodCache, currentPeriod })
    .catch((error) => showStatusError(`Unable to save report cache: ${error}`));
}

function initBackendRefreshPolling() {
  const poll = async () => {
    try {
      const revision = Number(await invokeTauri("get_refresh_revision"));
      if (!Number.isFinite(revision) || revision <= lastBackendRevisionMs) return;
      const snapshot = await invokeTauri("get_refresh_snapshot");
      if (snapshot?.report) {
        lastBackendRevisionMs = revision;
        await syncBackendRefresh(snapshot.report, Number(snapshot.refreshedAtMs));
      }
    } catch (_) {}
  };
  setTimeout(poll, 3000);
  setInterval(poll, 5000);
}

// Cache au démarrage (0 ms)
async function initColdStart() {
  try {
    quotaHistoryData = await loadQuotaHistory(invokeTauri);
    const cached = await invokeTauri("get_report_cache");
    if (cached?.periods && typeof cached.periods === "object") {
      Object.assign(periodCache, cached.periods);
      lastUpdatedTime = latestTimelineUpdatedAt(periodCache) || 0;
    }
    if (cached?.currentPeriod) {
      currentPeriod = getRestoredPeriod(cached.currentPeriod, Object.keys(PERIOD_LABELS));
      const select = document.getElementById("summary-period-select");
      if (select) select.value = currentPeriod;
    }
    computeDetectedAgents(periodCache.all?.reportData || cached?.summary, cached?.antigravity);
    if (cached && (cached.summary || cached.antigravity)) {
      if (cached.antigravity) antigravityData = cached.antigravity;
      const selectedCache = periodCache[currentPeriod];
      const startupSummary = mergeLiveSubscription(
        selectedCache?.reportData || cached.summary,
        latestLiveQuotaReport,
      );
      if (startupSummary) {
        reportData = startupSummary;
        fullReportData = startupSummary;
        computeDetectedAgents(reportData, antigravityData);
        renderHarnessTabs();
        updateTopBarQuotaPill(reportData);
        renderSummary();
      }
    }
  } catch (e) {
    console.debug("Pas de cache disponible au démarrage", e);
  }
}

async function preloadTimelines(refreshExisting = false) {
  const periods = timelinePreloadOrder(Object.keys(PERIOD_LABELS), currentPeriod);
  for (const period of periods) {
    if (period === "rtd") continue;
    if (!refreshExisting && !shouldRefreshTimelineInBackground(period, periodCache[period])) continue;
    try {
      await requestTimeline(period, refreshExisting);
      updateTimelineAvailability();
      await saveReportCache({ summary: reportData, periods: periodCache, currentPeriod });
    } catch (error) {
      // A missing source for one period must not prevent the other periods loading.
      console.debug(`Unable to preload ${period}`, error);
    }
  }
}

// ==========================================================================
// Détection stricte des harnais : conservation des harnais connus façon macOS
// ==========================================================================
function computeDetectedAgents(summary, agy) {
  if (summary && summary.agents && Array.isArray(summary.agents)) {
    for (const a of summary.agents) {
      const hasTokens = (a.totalTokens || 0) > 0;
      const hasCost = (a.totalCost || 0) > 0;
      const hasModels = a.models && a.models.length > 0;
      if (hasTokens || hasCost || hasModels) {
        allKnownAgents.add(a.agent.toLowerCase());
      }
    }
  }

  // Vérifier aussi les abonnements avec quota actif
  if (summary && summary.subscription && Array.isArray(summary.subscription.agents)) {
    for (const sub of summary.subscription.agents) {
      if (sub.liveLimits || sub.window != null) {
        allKnownAgents.add(sub.agent.toLowerCase());
      }
    }
  }

  // Google Antigravity
  if (agy && ((agy.total_tokens || 0) > 0 || (agy.total_cost || 0) > 0 || (agy.session_count || 0) > 0)) {
    allKnownAgents.add("antigravity");
  }

  detectedAgents = Array.from(allKnownAgents).sort();
}

// ==========================================================================
// Rendu dynamique des onglets (HarnessTabs) façon macOS avec menu More ▾
// ==========================================================================
function renderHarnessTabs() {
  const container = document.getElementById("harness-tabs");
  if (!container) return;

  container.innerHTML = "";

  // 1. Onglet permanent : General
  const generalBtn = document.createElement("button");
  generalBtn.className = "harness-tab" + (currentTab === "summary" ? " active" : "");
  generalBtn.dataset.tab = "summary";
  generalBtn.textContent = "General";
  generalBtn.addEventListener("click", () => switchTab("summary"));
  container.appendChild(generalBtn);

  // 2. Les agents détectés sous forme d'onglets directs (jusqu'à 8 pour inclure tous les agents actifs)
  const shownAgents = visibleAgents(detectedAgents, appSettings?.hiddenAgents);
  const maxDirectTabs = 8;
  const directAgents = shownAgents.slice(0, maxDirectTabs);
  const overflowAgents = shownAgents.slice(maxDirectTabs);

  directAgents.forEach((agent) => {
    const btn = document.createElement("button");
    btn.className = "harness-tab" + (currentTab === agent ? " active" : "");
    btn.dataset.tab = agent;
    btn.textContent = getAgentDisplayName(agent);
    btn.addEventListener("click", () => switchTab(agent));
    container.appendChild(btn);
  });

  // 3. Dropdown "More ▾" si d'autres agents détectés existent
  if (overflowAgents.length > 0) {
    const moreWrap = document.createElement("div");
    moreWrap.className = "more-dropdown-wrap";

    const isOverflowActive = overflowAgents.includes(currentTab);

    const moreBtn = document.createElement("button");
    moreBtn.className = "more-dropdown-btn" + (isOverflowActive ? " active" : "");
    moreBtn.textContent = (isOverflowActive ? getAgentDisplayName(currentTab) : "More") + " ▾";

    const moreMenu = document.createElement("div");
    moreMenu.className = "more-menu";

    overflowAgents.forEach((agent) => {
      const item = document.createElement("button");
      item.className = "more-menu-item";
      item.textContent = getAgentDisplayName(agent);
      item.addEventListener("click", (e) => {
        e.stopPropagation();
        moreMenu.classList.remove("open");
        switchTab(agent);
      });
      moreMenu.appendChild(item);
    });

    moreBtn.addEventListener("click", (e) => {
      e.stopPropagation();
      moreMenu.classList.toggle("open");
    });

    document.addEventListener("click", () => {
      moreMenu.classList.remove("open");
    });

    moreWrap.appendChild(moreBtn);
    moreWrap.appendChild(moreMenu);
    container.appendChild(moreWrap);
  }
}

function switchTab(tabId) {
  if (currentPeriod === "rtd" && tabId !== currentTab) {
    currentPeriod = "all";
    localStorage.setItem("agent-burn-period", currentPeriod);
    const cached = periodCache.all;
    if (cached) reportData = mergeLiveSubscription(cached.reportData, latestLiveQuotaReport);
  }
  currentTab = tabId;
  localStorage.setItem("agent-burn-tab", currentTab);

  // Mise à jour de la classe active sur les onglets
  renderHarnessTabs();

  // Masquer tous les panneaux
  document.querySelectorAll(".view-panel").forEach((p) => p.classList.remove("active"));

  if (tabId === "summary") {
    document.getElementById("view-summary")?.classList.add("active");
    renderSummary();
  } else if (tabId === "settings") {
    document.getElementById("view-settings")?.classList.add("active");
    renderSettings();
  } else {
    document.getElementById("view-harness")?.classList.add("active");
    renderHarnessView(tabId);
  }
}

// ==========================================================================
// Gestion de la timeline (Sélecteur de période façon macOS & 0 ms switcher)
// ==========================================================================
function initPeriods() {
  const summarySelect = document.getElementById("summary-period-select");
  if (summarySelect) {
    summarySelect.innerHTML = timelinePeriodEntries(PERIOD_LABELS, false)
      .map(([period, label]) => `<option value="${period}">${label}</option>`)
      .join("");
    summarySelect.value = currentPeriod;
    summarySelect.addEventListener("change", async (e) => {
      await switchPeriod(e.target.value);
    });
  }
}

async function switchPeriod(newPeriod) {
  currentPeriod = newPeriod;
  localStorage.setItem("agent-burn-period", currentPeriod);

  // Mise à jour de tous les dropdowns de l'interface
  const sSelect = document.getElementById("summary-period-select");
  if (sSelect) sSelect.value = currentPeriod;
  const hSelect = document.getElementById("harness-period-select");
  if (hSelect) hSelect.value = currentPeriod;

  const cacheKey = timelineCacheKey(currentPeriod);
  const selection = timelineSelection(periodCache, cacheKey, reportData, antigravityData);
  if (!selection.pending) {
    setTimelineRefreshing(false, "timeline");
    reportData = mergeLiveSubscription(selection.reportData, latestLiveQuotaReport);
    antigravityData = selection.antigravityData;
    computeDetectedAgents(reportData, antigravityData);
    renderHarnessTabs();
    if (currentTab === "summary") renderSummary();
    else if (currentTab !== "settings") renderHarnessView(currentTab);
    return;
  }

  setTimelineRefreshing(true, "timeline");
  await loadData(true);
}

function setTimelineRefreshing(refreshing, source = "timeline") {
  if (refreshing) activeRefreshSources.add(source);
  else activeRefreshSources.delete(source);
  const active = activeRefreshSources.size > 0;
  document.querySelectorAll(".timeline-refresh-status").forEach((status) => {
    status.hidden = !active;
  });
  const refreshButton = document.getElementById("refresh-btn");
  refreshButton?.classList.toggle("is-refreshing", active);
  refreshButton?.setAttribute("aria-busy", String(active));
}

function updateTimelineAvailability() {
  document.querySelectorAll(".macos-period-select").forEach((select) => {
    for (const option of select.options) {
      if (option.value !== "rtd") {
        option.disabled = false;
        continue;
      }
      const subscription = (reportData?.subscription?.agents || []).find(
        (agent) => agent.agent?.toLowerCase() === currentTab.toLowerCase(),
      );
      option.disabled = currentTab === "summary" || !resetWindowStartDate(
        subscription?.window?.resetDate,
        subscription?.window?.elapsedMinutes,
      );
    }
  });
}

function timelineCacheKey(period) {
  return period === "rtd" ? `rtd:${currentTab}` : period;
}

function requestTimeline(period, force = false) {
  const key = timelineCacheKey(period);
  if (!force && periodCache[key]) return Promise.resolve(periodCache[key]);
  if (periodRequests.has(key)) return periodRequests.get(key);

  let invocation;
  if (period === "rtd") {
    const subscription = (reportData?.subscription?.agents || []).find(
      (agent) => agent.agent?.toLowerCase() === currentTab.toLowerCase(),
    );
    const since = resetWindowStartDate(
      subscription?.window?.resetDate,
      subscription?.window?.elapsedMinutes,
    );
    if (!since) return Promise.reject(new Error("No reset window was detected for this harness."));
    invocation = invokeTauri("get_summary_since", { since });
  } else {
    invocation = invokeTauri("get_summary", { range: period === "all" ? null : period });
  }

  const request = invocation
    .then((summary) => {
      const entry = {
        reportData: JSON.parse(JSON.stringify(summary)),
        antigravityData: null,
        updatedAt: Date.now(),
      };
      periodCache[key] = entry;
      return entry;
    })
    .finally(() => periodRequests.delete(key));
  periodRequests.set(key, request);
  return request;
}

// ==========================================================================
// Chargement principal des données CLI
// ==========================================================================
async function loadData(force = false) {
  const cacheKey = timelineCacheKey(currentPeriod);
  const requestGeneration = summaryRequestGate.begin();
  if (!force && periodCache[cacheKey]) {
    const cached = periodCache[cacheKey];
    reportData = cached.reportData;
    antigravityData = cached.antigravityData;
    computeDetectedAgents(reportData, antigravityData);
    renderHarnessTabs();
    if (currentTab === "summary") renderSummary();
    else if (currentTab !== "settings") renderHarnessView(currentTab);
    return;
  }

  try {
    // 1. Récupération du résumé CLI et de l'historique des quotas
    const loaded = await requestTimeline(currentPeriod, force);
    try {
      quotaHistoryData = await invokeTauri("get_quota_history");
    } catch (_) {}

    if (!summaryRequestGate.isCurrent(requestGeneration) || timelineCacheKey(currentPeriod) !== cacheKey) return;

    reportData = mergeLiveSubscription(loaded.reportData, latestLiveQuotaReport);
    antigravityData = loaded.antigravityData;
    if (currentPeriod === "all" || !fullReportData) {
      fullReportData = loaded.reportData;
    }
    lastUpdatedTime = Date.now();

    // Antigravity est déjà normalisé dans le rapport standard par le backend Rust.
    // 4. Calcul strict des harnais détectés
    computeDetectedAgents(reportData, antigravityData);

    // 5. Mise à jour de la pilule de quota dans la barre supérieure
    updateTopBarQuotaPill(reportData);

    // 6. Rendu de l'UI
    renderHarnessTabs();

    if (currentTab === "summary") {
      renderSummary();
    } else if (currentTab === "settings") {
      renderSettings();
    } else {
      renderHarnessView(currentTab);
    }

    // Sauvegarde en cache disque
    saveReportCache({ summary: reportData, periods: periodCache, currentPeriod })
      .catch((error) => showStatusError(`Unable to save report cache: ${error}`));

  } catch (err) {
    showStatusError(`Unable to load usage: ${err}`);
  } finally {
    if (summaryRequestGate.isCurrent(requestGeneration)) setTimelineRefreshing(false, "timeline");
  }
}

async function performAllTimelineRefresh(showIndicator = false) {
  if (showIndicator) setTimelineRefreshing(true, "manual");
  try {
    const previousToday = periodCache.today;
    const summary = await invokeTauri("get_summary", { range: "today" });
    if (previousToday) periodCache.today = previousToday;
    updateCachedReportsFromToday(periodCache, summary);
    latestLiveQuotaReport = summary;
    lastUpdatedTime = Date.now();
    lastBackendRefreshMs = Math.max(lastBackendRefreshMs, lastUpdatedTime);

    const selected = periodCache[timelineCacheKey(currentPeriod)];
    if (selected) {
      reportData = mergeLiveSubscription(selected.reportData, latestLiveQuotaReport);
      computeDetectedAgents(reportData, antigravityData);
      renderHarnessTabs();
      updateTopBarQuotaPill(reportData);
      if (currentTab === "summary") renderSummary();
      else if (currentTab !== "settings") renderHarnessView(currentTab);
    }
    await saveReportCache({ summary: reportData, periods: periodCache, currentPeriod });
    void preloadTimelines();
  } catch (error) {
    showStatusError(`Unable to refresh timelines: ${error}`);
  } finally {
    if (showIndicator) setTimelineRefreshing(false, "manual");
  }
}

async function refreshStaleTimelinesAfterInitialBackend() {
  await waitForInitialRefresh(initialBackendRefresh);
  setTimelineRefreshing(false, "startup");
  const cacheKey = timelineCacheKey(currentPeriod);
  if (!isTimelineCacheFresh(periodCache[cacheKey])) {
    setTimelineRefreshing(true, "timeline");
    await loadData(true);
  }
  await preloadTimelines();
}

// Mise à jour de la pilule de quota dans le header
function updateTopBarQuotaPill(data) {
  const wrap = document.getElementById("quota-dropdown-wrap");
  const pillBtn = document.getElementById("quota-menu-pill");
  const pillText = document.getElementById("quota-pill-text");
  const chevron = document.getElementById("quota-pill-chevron");
  const menu = document.getElementById("quota-menu");
  const dot = document.querySelector("#quota-menu-pill .quota-dot");
  if (!pillText || !wrap) return;

  const agents = data?.subscription?.agents || [];
  // Filtrer uniquement les agents avec quota détecté
  const agentsWithQuota = agents.filter((agent) => quotaRemainingPercent(agent) !== null);

  // Si aucun quota n'est détecté, masquer totalement le tag
  if (agentsWithQuota.length === 0) {
    wrap.style.display = "none";
    return;
  }

  wrap.style.display = "inline-flex";

  // Trouver l'agent sélectionné, ou le premier disponible ayant un quota
  let selectedAgent = agentsWithQuota.find((a) => a.agent === appSettings?.quotaSource);
  if (!selectedAgent) {
    selectedAgent = agentsWithQuota[0];
  }

  const agentName = selectedAgent.agent;
  const displayName = getAgentDisplayName(agentName);
  const remaining = quotaRemainingPercent(selectedAgent);

  // Affichage : nom du harnais + pourcentage (ex: "Antigravity 93%")
  pillText.textContent = `${displayName} ${remaining.toFixed(0)}%`;
  if (dot) dot.className = "quota-dot live";

  // Menu déroulant si plusieurs agents avec quota
  if (agentsWithQuota.length > 1) {
    if (chevron) chevron.style.display = "inline";
    if (menu) {
      menu.innerHTML = "";
      agentsWithQuota.forEach((a) => {
        const item = document.createElement("button");
        const aDisp = getAgentDisplayName(a.agent);
        const aRem = quotaRemainingPercent(a);
        const isSelected = a.agent === selectedAgent.agent;
        item.className = `quota-menu-item${isSelected ? " selected" : ""}`;
        const label = document.createElement("span");
        label.textContent = `${isSelected ? "✓ " : ""}${aDisp}`;
        const value = document.createElement("span");
        value.className = "quota-menu-value";
        value.textContent = `${aRem.toFixed(0)}%`;
        item.append(label, value);
        item.addEventListener("click", async (e) => {
          e.stopPropagation();
          menu.classList.remove("open");
          try {
            const savedSettings = await persistQuotaSource(
              settingsLoadPromise,
              a.agent,
              (settings) => invokeTauri("set_settings", { settings }),
            );
            if (!savedSettings) return;
            appSettings = savedSettings;
            updateTopBarQuotaPill(data);
          } catch (error) {
            showStatusError(`Settings unavailable: ${error}`);
          }
        });
        menu.appendChild(item);
      });
    }

    pillBtn.onclick = (e) => {
      e.stopPropagation();
      menu?.classList.toggle("open");
    };
  } else {
    if (chevron) chevron.style.display = "none";
    if (menu) menu.innerHTML = "";
    pillBtn.onclick = null;
  }
}

// ==========================================================================
// Rendu de la vue "General" (All harnesses)
// ==========================================================================
function renderSummary() {
  if (!reportData) return;

  // 1. Sous-titre de période
  const periodLabel = PERIOD_LABELS[currentPeriod] || "All time";
  document.querySelectorAll(".period-subtitle").forEach((el) => {
    el.textContent = periodLabel;
  });

  const select = document.getElementById("summary-period-select");
  if (select) select.value = currentPeriod;

  // 2. Les 4 Cartes de métriques
  const totals = reportData.totals || {};
  const cost = totals.totalCost || 0;
  const tokens = totals.totalTokens || 0;
  const models = reportData.models || [];

  const spendEl = document.getElementById("summary-metric-spend");
  const tokensEl = document.getElementById("summary-metric-tokens");
  const tpdEl = document.getElementById("summary-metric-tokens-dollar");
  const modelsEl = document.getElementById("summary-metric-models");

  if (spendEl) spendEl.textContent = formatCurrency(cost);
  if (tokensEl) tokensEl.textContent = formatCompactTokens(tokens);
  if (tpdEl) tpdEl.textContent = formatTokensPerDollar(tokens, cost);
  if (modelsEl) modelsEl.textContent = models.length.toString();

  // 3. Configuration intelligente et écouteurs de granularité (Daily | Weekly | Monthly)
  setupGranularityPicker(
    "summary-granularity",
    "summary-activity-svg",
    "summary-chart-title",
    reportData.daily || [],
    null
  );
  renderActivityChart(
    "summary-activity-svg",
    "summary-chart-title",
    reportData.daily || [],
    null // null = couleur multi-agents
  );

  // 4. Liste "By harness"
  renderByHarnessList();

  // 5. Tableau des modèles
  renderModelsTable(models, cost, "summary-models-tbody", "summary-models-count", "summary-filter-input");

  const breakdownGrid = document.getElementById("summary-token-breakdown-grid");
  if (breakdownGrid) {
    const breakdown = aggregateTokenBreakdown(reportData.agents);
    breakdownGrid.innerHTML = visibleTokenBreakdownEntries(breakdown).map(([label, value]) => `
      <div class="token-metric-item">
        <span class="token-metric-label">${escapeHtml(label)}</span>
        <span class="token-metric-val">${formatCompactTokens(value || 0)}</span>
      </div>
    `).join("");
  }

  // 6. Abonnements
  renderSubscriptions();
}

// Rendu de la liste By harness
function renderByHarnessList() {
  const container = document.getElementById("by-harness-list");
  if (!container) return;

  container.innerHTML = "";

  const agents = (reportData?.agents || []).filter((a) => {
    return (a.totalCost || 0) > 0 || (a.totalTokens || 0) > 0;
  });

  if (agents.length === 0) {
    container.innerHTML = `<div style="color: var(--text-muted); font-size: 12px; padding: 12px;">No active harnesses in this period.</div>`;
    return;
  }

  agents.forEach((a) => {
    const row = document.createElement("div");
    row.className = "by-harness-row";
    row.innerHTML = `
      <div class="by-harness-row-left">
        <img class="by-harness-icon" src="${getAgentBrandIcon(a.agent)}" alt="${escapeHtml(a.agent)}" />
        <div class="by-harness-info">
          <span class="by-harness-name">${escapeHtml(getAgentDisplayName(a.agent))}</span>
          <span class="by-harness-tokens">${formatCompactTokens(a.totalTokens)} tokens</span>
        </div>
      </div>
      <span class="by-harness-spend">${formatCurrency(a.totalCost)}</span>
    `;

    // Cliquer sur un harnais bascule instantanément sur l'onglet de cet agent
    row.addEventListener("click", () => {
      switchTab(a.agent.toLowerCase());
    });

    container.appendChild(row);
  });
}

// ==========================================================================
// Rendu de la vue d'un Agent Spécifique (Codex, Cursor, etc.)
// ==========================================================================
function renderHarnessView(agent) {
  const container = document.getElementById("harness-view-content");
  if (!container || !reportData) return;

  let agentData = (reportData.agents || []).find((a) => a.agent.toLowerCase() === agent.toLowerCase());
  const subscriptionAgent = (reportData.subscription?.agents || fullReportData?.subscription?.agents || []).find(
    (a) => a.agent.toLowerCase() === agent.toLowerCase()
  );
  const subscriptionUi = subscriptionAgent ? subscriptionPresentation(subscriptionAgent) : null;

  const cost = agentData?.totalCost || 0;
  const tokens = agentData?.totalTokens || 0;
  const models = agentData?.models || [];
  const periodLabel = PERIOD_LABELS[currentPeriod] || "All time";
  const agentColor = getAgentColor(agent);

  let html = `
    <!-- En-tête de page -->
    <div class="page-header">
      <div class="header-titles">
        <div class="header-icon-box">
          <img src="${getAgentBrandIcon(agent)}" alt="${escapeHtml(agent)}" />
        </div>
        <div>
          <h1 class="page-title">${escapeHtml(getAgentDisplayName(agent))}</h1>
          <p class="page-subtitle">${periodLabel} · usage from your logs and connected providers</p>
        </div>
      </div>

      <div class="period-picker-box">
        <span class="timeline-refresh-status" hidden><span class="timeline-spinner" aria-hidden="true"></span>Refreshing</span>
        <select class="macos-period-select" id="harness-period-select">
          ${timelinePeriodEntries(PERIOD_LABELS)
            .map(([k, v]) => `<option value="${k}" ${k === currentPeriod ? "selected" : ""}>${v}</option>`)
            .join("")}
        </select>
      </div>
    </div>
  `;

  // 1. CARTE WEEKLY QUOTA (BURN-DOWN CHART) si l'agent a un quota (ex. Codex, Antigravity)
  const hasQuota = Boolean(subscriptionAgent && subscriptionAgent.window);
  if (hasQuota) {
    const quota = quotaPresentation(subscriptionAgent.window);
    const used = quota.usedPercent;
    const remaining = quota.remainingPercent;

    // Calculs de reset et d'allure 100% RÉELS basés sur les données CLI et l'archive
    const totalMinutes = 7 * 24 * 60; // 10,080 minutes hebdomadaires
    const minutesLeft = quota.resetInMinutes ?? Math.max(0, totalMinutes - (subscriptionAgent.window.elapsedMinutes || 0));
    const elapsed = subscriptionAgent.window.elapsedMinutes ?? Math.max(0, totalMinutes - minutesLeft);
    const daysLeft = Math.floor(minutesLeft / 1440);
    const hoursLeft = Math.floor((minutesLeft % 1440) / 60);
    const resetInText = `${daysLeft}d ${hoursLeft}h`;

    const resetDateObj = quota.resetDate || new Date(Date.now() + minutesLeft * 60 * 1000);
    const resetDateText = resetDateObj.toLocaleDateString("en-US", { month: "short", day: "numeric" }) + " · " + resetDateObj.toLocaleTimeString("en-US", { hour: "numeric", minute: "2-digit" });

    const idealRemaining = Math.max(0, 100 - (elapsed / totalMinutes) * 100);
    const paceDeltaVal = remaining - idealRemaining;
    const isAhead = paceDeltaVal >= -0.05;
    const paceDeltaText = Math.abs(paceDeltaVal) < 0.05
      ? "On pace"
      : (paceDeltaVal >= 0 ? "+" : "−") + Math.abs(paceDeltaVal).toFixed(1) + "% " + (isAhead ? "ahead" : "behind");
    const paceClass = isAhead ? "ahead" : "behind";
    const resetsAvail = subscriptionAgent.resetCreditsAvailable ?? 0;
    const dailyAllowance = (remaining / Math.max(1 / 1440, minutesLeft / 1440)).toFixed(1) + "% / day";

    const apiSpent = subscriptionAgent.window.apiEquivalentSpent || 0;
    const avgDollarsPerPct = "$" + (used > 0 ? (apiSpent / used).toFixed(2) : "0.00") + " / %";
    const avgTokensPerDollar = formatTokensPerDollar(tokens, cost);

    html += `
      <div class="groupbox burndown-grid-card">
        <!-- Colonne Gauche : Synthèse du Quota (1/3) -->
        <div class="quota-facts-col">
          <div class="quota-header-row">
            <span>Weekly quota</span>
            <span style="color: var(--text-muted); font-size: 13px;">🕒</span>
          </div>

          <div>
            <div class="quota-big-percent">${remaining.toFixed(1)}%</div>
            <div class="quota-remaining-label">remaining</div>
          </div>

          <div class="pace-badge ${paceClass}">${paceDeltaText}</div>

          <div class="quota-capsule-bar-bg">
            <div class="quota-capsule-bar-fill ${paceClass}" style="width: ${remaining}%;"></div>
          </div>

          <div class="quota-facts-list">
            <div class="quota-fact-row">
              <span class="quota-fact-label">Reset in</span>
              <div class="quota-fact-val-group">
                <span class="quota-fact-primary">${resetInText}</span>
                <span class="quota-fact-sub">${resetDateText}</span>
              </div>
            </div>
            <div class="quota-fact-row">
              <span class="quota-fact-label">Used</span>
              <div class="quota-fact-val-group">
                <span class="quota-fact-primary">${used.toFixed(1)}%</span>
                <span class="quota-fact-sub">since cycle start</span>
              </div>
            </div>
            <div class="quota-fact-row">
              <span class="quota-fact-label">Daily</span>
              <span class="quota-fact-primary">${dailyAllowance}</span>
            </div>
            <div class="quota-fact-row">
              <span class="quota-fact-label">Avg $ / %</span>
              <div class="quota-fact-val-group">
                <span class="quota-fact-primary">${avgDollarsPerPct}</span>
                <span class="quota-fact-sub">${avgTokensPerDollar}</span>
              </div>
            </div>
            <div class="quota-fact-row">
              <span class="quota-fact-label">Resets</span>
              <div class="quota-fact-val-group">
                <span class="quota-fact-primary">${resetsAvail}</span>
                <span class="quota-fact-sub">banked</span>
              </div>
            </div>
            ${subscriptionAgent.shortWindow ? (() => {
              const sw = quotaPresentation(subscriptionAgent.shortWindow);
              const swMins = sw.resetInMinutes;
              const swTimeText = swMins != null ? (swMins >= 60 ? `${Math.floor(swMins / 60)}h ${swMins % 60}m` : `${swMins}m`) : "";
              return `
                <div class="quota-fact-row">
                  <span class="quota-fact-label">5-hour limit</span>
                  <div class="quota-fact-val-group">
                    <span class="quota-fact-primary">${sw.remainingPercent.toFixed(1)}%</span>
                    <span class="quota-fact-sub">${swTimeText ? `resets in ${swTimeText}` : "session limit"}</span>
                  </div>
                </div>
              `;
            })() : ""}
          </div>
        </div>

        <!-- Colonne Droite : Le Burn-Down Chart SVG (2/3) -->
        <div class="burndown-chart-col">
          <div class="burndown-top-bar">
            <div class="burndown-legend" id="burndown-legend-container">
            </div>

            <div class="burndown-right-controls">
              <select class="macos-select" id="quota-horizon-select">
                <option value="rte" ${currentQuotaHorizon === "rte" ? "selected" : ""}>Until reset</option>
                <option value="rtd" ${currentQuotaHorizon === "rtd" ? "selected" : ""}>Reset to today</option>
                <option value="today" ${currentQuotaHorizon === "today" ? "selected" : ""}>Today</option>
                <option value="week" ${currentQuotaHorizon === "week" ? "selected" : ""}>Last 7 days</option>
                <option value="month" ${currentQuotaHorizon === "month" ? "selected" : ""}>Last 30 days</option>
              </select>
              <span class="projected-time-text">Projected · ${resetDateText}</span>
            </div>
          </div>

          <div class="burndown-svg-wrapper" id="burndown-svg-wrapper">
            <!-- Rendu interactif du Burn-Down SVG -->
          </div>
        </div>
      </div>
    `;
  }

  // 2. Les 4 Cartes de métriques de l'agent
  html += `
    <div class="groupbox metrics-groupbox">
      <div class="metric-col">
        <span class="metric-title">Total spend</span>
        <span class="metric-value">${formatCurrency(cost)}</span>
        <span class="metric-detail">API-equivalent value</span>
      </div>
      <div class="metric-divider"></div>
      <div class="metric-col">
        <span class="metric-title">Tokens</span>
        <span class="metric-value">${formatCompactTokens(tokens)}</span>
        <span class="metric-detail">Input, output and cache</span>
      </div>
      <div class="metric-divider"></div>
      <div class="metric-col">
        <span class="metric-title">Avg tokens / $</span>
        <span class="metric-value">${formatTokensPerDollar(tokens, cost)}</span>
        <span class="metric-detail">${periodLabel}</span>
      </div>
      <div class="metric-divider"></div>
      <div class="metric-col">
        <span class="metric-title">Models</span>
        <span class="metric-value">${models.length}</span>
        <span class="metric-detail">${escapeHtml(getAgentDisplayName(agent))}</span>
      </div>
    </div>

    <!-- Histogramme d'activité de l'agent -->
    <div class="groupbox activity-card" style="margin-bottom: 20px;">
      <div class="chart-top-bar">
        <span class="chart-heading" id="harness-chart-title">Weekly spend</span>
        <div class="granularity-pill-group" id="harness-granularity">
          <button class="gran-btn" data-gran="daily">Daily</button>
          <button class="gran-btn active" data-gran="weekly">Weekly</button>
          <button class="gran-btn" data-gran="monthly">Monthly</button>
        </div>
        <span class="currency-label">USD</span>
      </div>
      <div class="chart-svg-container" id="harness-activity-svg"></div>
    </div>

    <!-- Tableau des modèles de l'agent -->
    <div class="groupbox models-card">
      <div class="models-header-row">
        <div class="models-title-wrap">
          <span class="card-heading">Models · available source data</span>
          <span class="count-badge">${models.length}</span>
        </div>
        <div class="search-wrap">
          <input type="text" class="macos-filter-input" id="harness-filter-input" placeholder="Filter models" />
        </div>
      </div>
      <div class="table-wrap">
        <table class="macos-table" id="harness-models-table">
          <thead>
            <tr>
              <th class="col-model">Model</th>
              <th class="col-token-part text-right sortable" data-sort="input">Input <span class="sort-indicator"></span></th>
              <th class="col-token-part text-right sortable" data-sort="cacheRead">Cached input <span class="sort-indicator"></span></th>
              <th class="col-token-part text-right sortable" data-sort="cacheWrite">Cache write <span class="sort-indicator"></span></th>
              <th class="col-token-part text-right sortable" data-sort="output">Output <span class="sort-indicator"></span></th>
              <th class="col-spend text-right sortable" data-sort="spend">Spend <span class="sort-indicator">↓</span></th>
              <th class="col-share text-right sortable" data-sort="share">Share <span class="sort-indicator"></span></th>
            </tr>
          </thead>
          <tbody id="harness-models-tbody"></tbody>
        </table>
      </div>
    </div>
  `;

  // 3. Token Breakdown (Capture d'écran de l'utilisateur)
  if (agentData?.tokenBreakdown) {
    const breakdownItems = visibleTokenBreakdownEntries(agentData.tokenBreakdown);
    html += `
      <div class="groupbox token-breakdown-card">
        <h2 class="card-heading">Token breakdown · available source data</h2>
        <div class="token-breakdown-grid">
          ${breakdownItems.map(([label, value]) => `
            <div class="token-metric-item">
              <span class="token-metric-label">${escapeHtml(label)}</span>
              <span class="token-metric-val">${formatCompactTokens(value || 0)}</span>
            </div>`).join("")}
        </div>
      </div>
    `;
  }

  // 4. Subscription economics, token costs and weekly trends (Accordéon dépliable)
  if (subscriptionAgent && subscriptionUi.monthlyPrice !== null) {
    const est = subscriptionAgent.estimate || {};
    const planCost = subscriptionUi.monthlyPrice;
    const monthlyVal = est.monthlyValue || cost;
    const valueMult = est.valueMultiple ? est.valueMultiple.toFixed(2) + "×" : "—";
    const subsidy = Math.max(0, monthlyVal - planCost);
    const discountPct = monthlyVal > 0 ? ((subsidy / monthlyVal) * 100).toFixed(1) + "%" : "—";

    html += `
      <details class="groupbox disclosure-box">
        <summary class="disclosure-summary">
          <span>Subscription economics, token costs and weekly trends</span>
          <span class="disclosure-arrow">▾</span>
        </summary>
        <div class="disclosure-content">
          <div class="econ-metrics-row">
            <div class="metric-col">
              <span class="metric-title">Past 30 days</span>
              <span class="metric-value">${formatCurrency(monthlyVal)}</span>
              <span class="metric-detail">API-equivalent value</span>
            </div>
            <div class="metric-divider"></div>
            <div class="metric-col">
              <span class="metric-title">Monthly plan</span>
              <span class="metric-value">${formatCurrency(planCost)}</span>
          <span class="metric-detail">${escapeHtml(subscriptionUi.plan)} plan</span>
            </div>
            <div class="metric-divider"></div>
            <div class="metric-col">
              <span class="metric-title">Subscription value</span>
              <span class="metric-value">${valueMult}</span>
              <span class="metric-detail">Usage / monthly price</span>
            </div>
          </div>
          <div class="econ-sub-row">
            <span>API-equivalent minus plan: ${formatCurrency(subsidy)}</span>
            <span>API pricing discount: ${discountPct}</span>
          </div>
        </div>
      </details>
    `;
  }

  // 5. Subscriptions Pill
  if (subscriptionAgent) {
    html += `
      <div class="subscriptions-section">
        <h2 class="card-heading" style="margin-bottom: 8px;">Subscriptions</h2>
        <div class="subscriptions-pills-row">
          <div class="sub-pill">
            <strong>${escapeHtml(getAgentDisplayName(agent))} · ${escapeHtml(subscriptionUi.plan)}</strong>
            <span style="color: var(--text-muted);">·</span>
            <span>${subscriptionUi.monthlyPrice === null ? "Monthly price unavailable" : `${formatCurrency(subscriptionUi.monthlyPrice)}/mo`}</span>
          </div>
        </div>
      </div>
    `;
  }

  // 6. Note légale
  html += `
    <div class="disclaimer-note">
      <span>ⓘ</span>
      <span>Spend is API-equivalent usage, not your subscription bill.</span>
    </div>
  `;

  container.innerHTML = html;

  // Listeners pour l'agent
  document.getElementById("harness-period-select")?.addEventListener("change", async (e) => {
    await switchPeriod(e.target.value);
  });
  updateTimelineAvailability();

  // Granularité d'activité adaptée et rendu de l'histogramme de l'agent
  const agentDays = agentData?.daily || [];
  setupGranularityPicker("harness-granularity", "harness-activity-svg", "harness-chart-title", agentDays, agentColor);
  renderActivityChart("harness-activity-svg", "harness-chart-title", agentDays, agentColor);

  // Rendu du tableau des modèles
  renderModelsTable(models, cost, "harness-models-tbody", null, "harness-filter-input");

  // Rendu du Burn-Down SVG avec vraies données
  if (hasQuota) {
    renderBurndownSVG("burndown-svg-wrapper", subscriptionAgent);
  }
}

// État d'horizon de la carte quota (5 timelines macOS : rte, rtd, today, week, month)
let currentQuotaHorizon = "rte";

// ==========================================================================
// Rendu SVG du Burn-Down Chart (Interactif, 5 Timelines & Survol souris)
// Conditionnalité 100% conforme à macOS Swift (showsForecast et showsIdeal)
// ==========================================================================
function renderBurndownSVG(containerId, subAgent) {
  const container = document.getElementById(containerId);
  if (!container) return;

  // Écouteur de changement d'horizon si le sélecteur existe
  const horizonSelect = document.getElementById("quota-horizon-select");
  if (horizonSelect && !horizonSelect.dataset.listenerAttached) {
    horizonSelect.dataset.listenerAttached = "true";
    horizonSelect.value = currentQuotaHorizon;
    horizonSelect.addEventListener("change", (e) => {
      currentQuotaHorizon = e.target.value;
      renderBurndownSVG(containerId, subAgent);
    });
  }

  const width = container.clientWidth || 600;
  const height = 220;
  const padL = 40;
  const padR = 30;
  const padT = 20;
  const padB = 30;

  const chartW = width - padL - padR;
  const chartH = height - padT - padB;

  const used = subAgent?.window?.usedPercent || 0;
  const remaining = Math.max(0, 100 - used);
  const elapsed = Math.max(1, subAgent?.window?.elapsedMinutes || 1000);
  const agentName = subAgent?.agent || currentTab;

  // Règles exactes de QuotaChart.swift macOS :
  // showsForecast est true UNIQUEMENT pour 'rte' (Until reset)
  // showsIdeal (Pace) est true UNIQUEMENT pour 'rte' et 'rtd' (Reset to today)
  const showsForecast = currentQuotaHorizon === "rte";
  const showsIdeal = currentQuotaHorizon === "rte" || currentQuotaHorizon === "rtd";

  // Définition de l'échelle temporelle selon l'horizon sélectionné
  const nowTime = new Date();
  let cycleEndTime = subAgent?.window?.resetDate ? new Date(subAgent.window.resetDate) : new Date(nowTime.getTime() + (7 * 24 * 60 - elapsed) * 60000);
  if (isNaN(cycleEndTime.getTime())) {
    cycleEndTime = new Date(nowTime.getTime() + (7 * 24 * 60 - elapsed) * 60000);
  }

  const cycleDurationMins = 7 * 24 * 60; // 7 jours pour le cycle complet
  const cycleStartTime = new Date(cycleEndTime.getTime() - cycleDurationMins * 60000);

  let windowStart = cycleStartTime;
  let windowEnd = cycleEndTime;

  if (currentQuotaHorizon === "rtd") {
    windowStart = cycleStartTime;
    windowEnd = nowTime;
  } else if (currentQuotaHorizon === "today") {
    windowStart = new Date(nowTime.getFullYear(), nowTime.getMonth(), nowTime.getDate());
    windowEnd = nowTime;
  } else if (currentQuotaHorizon === "week") {
    windowStart = new Date(nowTime.getTime() - 7 * 24 * 60 * 60000);
    windowEnd = nowTime;
  } else if (currentQuotaHorizon === "month") {
    windowStart = new Date(nowTime.getTime() - 30 * 24 * 60 * 60000);
    windowEnd = nowTime;
  }

  const windowDurationMs = Math.max(1000, windowEnd.getTime() - windowStart.getTime());

  function formatMacChartDate(d) {
    if (!d || isNaN(d.getTime())) return "";
    const months = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    const m = months[d.getMonth()];
    const dayNum = d.getDate();
    let h = d.getHours();
    const mins = d.getMinutes().toString().padStart(2, "0");
    const ampm = h >= 12 ? "PM" : "AM";
    h = h % 12 || 12;
    return `${m} ${dayNum} at ${h}:${mins} ${ampm}`;
  }

  // Noms réels des repères pour l'axe X (façon macOS)
  let daysNames = [];
  const numGridX = (currentQuotaHorizon === "rte" || currentQuotaHorizon === "week") ? 8 : 7;
  for (let i = 0; i < numGridX; i++) {
    const d = new Date(windowStart.getTime() + (i / (numGridX - 1)) * windowDurationMs);
    if (currentQuotaHorizon === "today") {
      daysNames.push(`${d.getHours().toString().padStart(2, "0")}:${d.getMinutes().toString().padStart(2, "0")}`);
    } else if (currentQuotaHorizon === "month") {
      const months = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
      daysNames.push(`${months[d.getMonth()]} ${d.getDate()}`);
    } else {
      daysNames.push(d.toLocaleDateString("en-US", { weekday: "short" }));
    }
  }

  // Échelle Y : 0% à 100%
  const yToCoord = (pct) => padT + chartH * (1 - Math.max(0, Math.min(100, pct)) / 100);

  // Conversion temps -> coordonnée X dans la fenêtre visible
  const timeToX = (d) => {
    const t = d instanceof Date ? d.getTime() : new Date(d).getTime();
    const ratio = Math.max(0, Math.min(1, (t - windowStart.getTime()) / windowDurationMs));
    return padL + ratio * chartW;
  };

  // Position actuelle
  const curX = Math.max(padL, Math.min(padL + chartW, timeToX(nowTime)));
  const curY = yToCoord(remaining);

  // Rythme idéal (Pace) : de 100% au début du cycle à 0% au reset
  const cycleTotalMs = Math.max(1000, cycleEndTime.getTime() - cycleStartTime.getTime());
  const curCycleElapsedMs = Math.max(0, Math.min(cycleTotalMs, nowTime.getTime() - cycleStartTime.getTime()));
  const currentIdeal = Math.max(0, 100 - (curCycleElapsedMs / cycleTotalMs) * 100);
  const paceDelta = remaining - currentIdeal;
  const isAhead = paceDelta >= -0.05;
  const lineColor = isAhead ? "var(--green-ahead)" : "var(--red-behind)";

  const paceX1 = timeToX(cycleStartTime);
  const paceY1 = yToCoord(100);
  const paceX2 = timeToX(cycleEndTime);
  const paceY2 = yToCoord(0);

  // Projection Forecast fidèle à macOS (Usage.swift:340-353)
  const elapsedFraction = Math.max(0.0001, curCycleElapsedMs / cycleTotalMs);
  const usedPercent = Math.max(0, 100 - remaining);
  const projectedUse = subAgent?.estimate?.projectedUsePercent != null
    ? subAgent.estimate.projectedUsePercent
    : (usedPercent > 0 && elapsedFraction > 0 ? usedPercent / elapsedFraction : 0);

  const projectedRemaining = Math.max(0, 100 - projectedUse);

  // Date et position où le quota atteint 0% si la consommation est excessive (>100%)
  const projectedEndMs = projectedUse > 100
    ? cycleStartTime.getTime() + cycleTotalMs * (100 / projectedUse)
    : cycleEndTime.getTime();
  const projectedEndDate = new Date(projectedEndMs);
  const projEndX = Math.min(paceX2, Math.max(curX, timeToX(projectedEndDate)));

  // Extraction et filtrage des échantillons réels de l'historique des quotas
  const rawSamples = [];
  if (Array.isArray(quotaHistoryData)) {
    for (const entry of quotaHistoryData) {
      if (!entry?.timestamp || !Array.isArray(entry.agents)) continue;
      const d = new Date(entry.timestamp);
      if (isNaN(d.getTime())) continue;
      const match = entry.agents.find((a) => a.agent === agentName);
      if (match && typeof match.remainingPercent === "number") {
        rawSamples.push({
          date: d,
          remaining: Math.max(0, Math.min(100, match.remainingPercent)),
        });
      }
    }
  }
  rawSamples.sort((a, b) => a.date.getTime() - b.date.getTime());

  // Filtrer les échantillons dans l'intervalle visible [windowStart, windowEnd]
  let inWindow = rawSamples.filter((s) => s.date >= windowStart && s.date <= windowEnd);

  if (showsIdeal) {
    // Si un reset survient (quota remonte de plus de 1%), on conserve depuis le dernier reset
    let lastResetIdx = 0;
    for (let i = 1; i < inWindow.length; i++) {
      if (inWindow[i].remaining > inWindow[i - 1].remaining + 1.0) {
        lastResetIdx = i;
      }
    }
    if (lastResetIdx > 0) {
      inWindow = inWindow.slice(lastResetIdx);
    }
    // Ancrer à 100% au début du cycle si aucun relevé précoce
    if (inWindow.length === 0 || inWindow[0].date.getTime() > windowStart.getTime() + 90000) {
      inWindow.unshift({ date: windowStart, remaining: 100 });
    }
  } else {
    if (inWindow.length === 0) {
      inWindow.push({ date: windowStart, remaining });
    }
  }

  // Ancrer le relevé en direct à nowTime
  const lastSample = inWindow[inWindow.length - 1];
  if (!lastSample || Math.abs(lastSample.date.getTime() - nowTime.getTime()) > 30000) {
    inWindow.push({ date: nowTime, remaining });
  } else {
    inWindow[inWindow.length - 1] = { date: nowTime, remaining };
  }

  // Marches d'escalier fidèles à macOS (holds plateau then steps)
  const drawnSamples = [];
  if (inWindow.length > 0) {
    let prev = inWindow[0];
    drawnSamples.push(prev);
    const stepGapMs = 2 * 3600 * 1000;
    for (let i = 1; i < inWindow.length; i++) {
      const s = inWindow[i];
      const dt = s.date.getTime() - prev.date.getTime();
      if (dt > 0 && dt <= stepGapMs && Math.abs(s.remaining - prev.remaining) >= 0.05) {
        drawnSamples.push({ date: s.date, remaining: prev.remaining });
      }
      drawnSamples.push(s);
      prev = s;
    }
  }

  const drawnPoints = drawnSamples.map((s) => ({
    x: timeToX(s.date),
    y: yToCoord(s.remaining),
    remaining: s.remaining,
    date: s.date,
  }));

  // Initialisation du texte temporel en haut à droite
  const projectedText = container.closest(".burndown-chart-col")?.querySelector(".projected-time-text");
  if (projectedText) {
    projectedText.textContent = `Recorded · ${formatMacChartDate(nowTime)}`;
  }

  // Synchronisation dynamique de la légende en haut selon l'horizon
  const burndownCol = container.closest(".burndown-chart-col");
  const legendEl = burndownCol?.querySelector(".burndown-legend");
  if (legendEl) {
    let legendHtml = `
      <div class="legend-item recorded ${isAhead ? 'ahead' : 'behind'}" id="burndown-legend-recorded">
        <span class="legend-stroke" style="background: ${isAhead ? 'var(--green-ahead)' : 'var(--red-behind)'};"></span>
        <span class="legend-label-text">Recorded ${remaining.toFixed(1)}%</span>
      </div>
    `;
    if (showsForecast) {
      legendHtml += `
        <div class="legend-item" id="burndown-legend-forecast">
          <span class="legend-stroke dashed" style="color: var(--text-muted);"></span>
          <span class="legend-label-text">Forecast ${projectedRemaining.toFixed(1)}%</span>
        </div>
      `;
    }
    if (showsIdeal) {
      legendHtml += `
        <div class="legend-item" id="burndown-legend-pace">
          <span class="legend-stroke dashed" style="color: var(--text-muted);"></span>
          <span class="legend-label-text">Pace ${currentIdeal.toFixed(1)}%</span>
        </div>
      `;
    }
    legendEl.innerHTML = legendHtml;
  }

  // 1. Grille horizontale Y
  let yGridSvg = "";
  [0, 25, 50, 75, 100].forEach((val) => {
    const y = yToCoord(val);
    yGridSvg += `
      <line x1="${padL}" y1="${y}" x2="${padL + chartW}" y2="${y}" stroke="rgba(255,255,255,0.06)" stroke-dasharray="3,5" stroke-width="0.8" />
      <text x="${padL - 8}" y="${y + 4}" fill="#8e8e93" font-size="10" text-anchor="end" font-family="monospace">${val}%</text>
    `;
  });

  // 2. Grille verticale X
  let xGridSvg = "";
  daysNames.forEach((dName, i) => {
    const x = padL + (i / (daysNames.length - 1)) * chartW;
    xGridSvg += `
      <line x1="${x}" y1="${padT}" x2="${x}" y2="${padT + chartH}" stroke="rgba(255,255,255,0.04)" stroke-width="1" />
      <text x="${x}" y="${padT + chartH + 18}" fill="#8e8e93" font-size="10" text-anchor="middle">${dName}</text>
    `;
  });

  // 3. Bande de position actuelle (uniquement si showsIdeal)
  const dayBandSvg = showsIdeal ? `
    <rect x="${Math.max(padL, curX - 14)}" y="${padT}" width="28" height="${chartH}" fill="${isAhead ? 'rgba(34, 197, 94, 0.08)' : 'rgba(239, 68, 68, 0.08)'}" />
  ` : "";

  // 4. Ligne de Pace (uniquement si showsIdeal)
  const paceSvg = showsIdeal ? `
    <line x1="${paceX1}" y1="${paceY1}" x2="${paceX2}" y2="${paceY2}" stroke="#8e8e93" stroke-dasharray="4,4" stroke-width="1.5" />
  ` : "";

  // 5. Tracé Recorded réel (avec historique)
  let pathD = "";
  if (drawnPoints.length > 0) {
    pathD = `M ${drawnPoints[0].x.toFixed(1)} ${drawnPoints[0].y.toFixed(1)}`;
    for (let i = 1; i < drawnPoints.length; i++) {
      pathD += ` L ${drawnPoints[i].x.toFixed(1)} ${drawnPoints[i].y.toFixed(1)}`;
    }
  } else {
    pathD = `M ${padL} ${yToCoord(100)} L ${curX} ${curY}`;
  }

  const firstX = drawnPoints.length > 0 ? drawnPoints[0].x.toFixed(1) : padL;
  const recordedAreaSvg = !showsIdeal ? `
    <path d="${pathD} L ${curX.toFixed(1)} ${padT + chartH} L ${firstX} ${padT + chartH} Z" fill="${lineColor}" opacity="0.08" />
  ` : "";

  // 6. Projection Forecast (si applicable pour l'horizon : rte uniquement)
  let forecastSvg = "";
  if (showsForecast) {
    if (projectedUse > 100) {
      forecastSvg = `
        <line x1="${curX.toFixed(1)}" y1="${curY.toFixed(1)}" x2="${projEndX.toFixed(1)}" y2="${yToCoord(0).toFixed(1)}" stroke="${lineColor}" stroke-dasharray="5,5" stroke-width="2" opacity="0.8" />
      `;
    } else {
      forecastSvg = `
        <line x1="${curX.toFixed(1)}" y1="${curY.toFixed(1)}" x2="${paceX2.toFixed(1)}" y2="${yToCoord(projectedRemaining).toFixed(1)}" stroke="${lineColor}" stroke-dasharray="5,5" stroke-width="2" opacity="0.8" />
      `;
    }
  }

  // 7. Ligne de Reset (rte uniquement)
  const resetSvg = showsForecast ? `
    <line x1="${paceX2}" y1="${padT}" x2="${paceX2}" y2="${padT + chartH}" stroke="#8e8e93" stroke-dasharray="3,3" stroke-width="1" />
    <text x="${paceX2 - 6}" y="${padT + 12}" fill="#8e8e93" font-size="9.5" text-anchor="end">Reset</text>
  ` : "";

  // 8. Curseur interactif
  const deltaBadgeText = (paceDelta >= 0 ? "+" : "") + paceDelta.toFixed(1) + "%";

  container.innerHTML = `
    <svg id="burndown-chart-svg" width="100%" height="${height}" viewBox="0 0 ${width} ${height}" style="overflow: visible; cursor: crosshair;">
      ${dayBandSvg}
      ${yGridSvg}
      ${xGridSvg}
      ${paceSvg}
      ${recordedAreaSvg}
      ${forecastSvg}
      ${resetSvg}
      <path d="${pathD}" fill="none" stroke="${lineColor}" stroke-width="2.5" stroke-linejoin="round" />
      <line id="burndown-hover-line" x1="${curX}" y1="${padT}" x2="${curX}" y2="${padT + chartH}" stroke="rgba(255,255,255,0.3)" stroke-width="1" stroke-dasharray="2,3" />
      <circle id="burndown-hover-dot" cx="${curX}" cy="${curY}" r="4.5" fill="${lineColor}" />
      <g id="burndown-hover-badge" transform="translate(${Math.min(chartW - 80, curX + 10)}, ${Math.max(padT + 10, curY - 12)})">
        <rect x="0" y="0" width="${showsIdeal ? 125 : 65}" height="22" rx="11" fill="rgba(28, 28, 32, 0.95)" stroke="rgba(255,255,255,0.2)" stroke-width="0.5" />
        <text id="burndown-badge-pct" x="12" y="15" fill="#ffffff" font-size="10.5" font-weight="600" font-family="monospace">${remaining.toFixed(1)}%</text>
        ${showsIdeal ? `
          <text x="56" y="15" fill="#8e8e93" font-size="10">·</text>
          <text id="burndown-badge-delta" x="66" y="15" fill="${lineColor}" font-size="10.5" font-weight="600">${deltaBadgeText}</text>
        ` : ""}
      </g>
    </svg>
  `;

  // Gestion du survol interactif souris sur le Burn-Down Chart (Façon macOS)
  const svgEl = document.getElementById("burndown-chart-svg");
  const hoverDot = document.getElementById("burndown-hover-dot");
  const hoverBadge = document.getElementById("burndown-hover-badge");
  const hoverLine = document.getElementById("burndown-hover-line");
  const badgePct = document.getElementById("burndown-badge-pct");
  const badgeDelta = document.getElementById("burndown-badge-delta");

  if (svgEl) {
    svgEl.addEventListener("mousemove", (e) => {
      const rect = svgEl.getBoundingClientRect();
      const rawMouseX = ((e.clientX - rect.left) / rect.width) * width;
      const mouseX = Math.max(padL, Math.min(showsForecast ? padL + chartW : curX, rawMouseX));
      const ratio = (mouseX - padL) / chartW;

      let curVal;
      if (mouseX <= curX && drawnPoints.length >= 2) {
        let pA = drawnPoints[0];
        let pB = drawnPoints[drawnPoints.length - 1];
        for (let i = 0; i < drawnPoints.length - 1; i++) {
          if (mouseX >= drawnPoints[i].x && mouseX <= drawnPoints[i + 1].x) {
            pA = drawnPoints[i];
            pB = drawnPoints[i + 1];
            break;
          }
        }
        const segW = pB.x - pA.x;
        if (segW > 0.001) {
          const t = (mouseX - pA.x) / segW;
          curVal = pA.remaining + t * (pB.remaining - pA.remaining);
        } else {
          curVal = pB.remaining;
        }
      } else if (mouseX <= curX) {
        curVal = remaining;
      } else {
        if (projectedUse > 100) {
          if (mouseX <= projEndX) {
            const spanX = Math.max(0.001, projEndX - curX);
            const progress = (mouseX - curX) / spanX;
            curVal = Math.max(0, remaining - progress * remaining);
          } else {
            curVal = 0;
          }
        } else {
          const spanX = Math.max(0.001, paceX2 - curX);
          const progress = (mouseX - curX) / spanX;
          curVal = Math.max(0, remaining + progress * (projectedRemaining - remaining));
        }
      }

      const curYPos = yToCoord(curVal);
      const hoverTime = new Date(windowStart.getTime() + ratio * windowDurationMs);
      const idealVal = showsIdeal
        ? Math.max(0, 100 - ((hoverTime.getTime() - cycleStartTime.getTime()) / cycleTotalMs) * 100)
        : null;
      const delta = idealVal != null ? curVal - idealVal : null;
      const ahead = delta != null ? delta >= -0.05 : true;
      const tone = ahead ? "var(--green-ahead)" : "var(--red-behind)";

      if (hoverLine) {
        hoverLine.setAttribute("x1", mouseX);
        hoverLine.setAttribute("x2", mouseX);
      }
      if (hoverDot) {
        hoverDot.setAttribute("cx", mouseX);
        hoverDot.setAttribute("cy", curYPos);
        hoverDot.setAttribute("fill", tone);
      }
      if (hoverBadge) {
        const badgeWidth = showsIdeal ? 125 : 65;
        const bx = Math.min(chartW - badgeWidth + 20, mouseX + 10);
        const by = Math.max(padT + 10, curYPos - 12);
        hoverBadge.setAttribute("transform", `translate(${bx}, ${by})`);
      }
      if (badgePct) badgePct.textContent = `${curVal.toFixed(1)}%`;
      if (badgeDelta && showsIdeal && delta != null) {
        badgeDelta.textContent = `${delta >= 0 ? '+' : ''}${delta.toFixed(1)}%`;
        badgeDelta.setAttribute("fill", tone);
      }

      // Date dynamique en haut à droite façon macOS
      const isObserved = mouseX <= curX;
      if (projectedText) {
        projectedText.textContent = isObserved
          ? `Recorded · ${formatMacChartDate(hoverTime)}`
          : `Projected · ${formatMacChartDate(hoverTime)}`;
      }

      // Légende dynamique en temps réel
      const recLabel = document.querySelector("#burndown-legend-recorded .legend-label-text");
      const fcastLabel = document.querySelector("#burndown-legend-forecast .legend-label-text");
      const paceLabel = document.querySelector("#burndown-legend-pace .legend-label-text");
      if (recLabel && isObserved) recLabel.textContent = `Recorded ${curVal.toFixed(1)}%`;
      if (fcastLabel && !isObserved) fcastLabel.textContent = `Forecast ${curVal.toFixed(1)}%`;
      if (paceLabel && idealVal != null) paceLabel.textContent = `Pace ${idealVal.toFixed(1)}%`;
    });

    svgEl.addEventListener("mouseleave", () => {
      if (hoverLine) {
        hoverLine.setAttribute("x1", curX);
        hoverLine.setAttribute("x2", curX);
      }
      if (hoverDot) {
        hoverDot.setAttribute("cx", curX);
        hoverDot.setAttribute("cy", curY);
        hoverDot.setAttribute("fill", lineColor);
      }
      if (hoverBadge) {
        const bx = Math.min(chartW - (showsIdeal ? 125 : 65) + 20, curX + 10);
        const by = Math.max(padT + 10, curY - 12);
        hoverBadge.setAttribute("transform", `translate(${bx}, ${by})`);
      }
      if (badgePct) badgePct.textContent = `${remaining.toFixed(1)}%`;
      if (badgeDelta && showsIdeal) {
        badgeDelta.textContent = deltaBadgeText;
        badgeDelta.setAttribute("fill", lineColor);
      }
      if (projectedText) {
        projectedText.textContent = `Recorded · ${formatMacChartDate(nowTime)}`;
      }

      const recLabel = document.querySelector("#burndown-legend-recorded .legend-label-text");
      const fcastLabel = document.querySelector("#burndown-legend-forecast .legend-label-text");
      const paceLabel = document.querySelector("#burndown-legend-pace .legend-label-text");
      if (recLabel) recLabel.textContent = `Recorded ${remaining.toFixed(1)}%`;
      if (fcastLabel) fcastLabel.textContent = `Forecast ${projectedRemaining.toFixed(1)}%`;
      if (paceLabel) paceLabel.textContent = `Pace ${currentIdeal.toFixed(1)}%`;
    });
  }
}

// ==========================================================================
// Vrai Regroupement Temporel (bucketDailyUsage sans aucun mock)
// ==========================================================================
function bucketDailyUsage(days, granularity) {
  if (!days || days.length === 0) return [];
  if (granularity === "daily") {
    return [...days].sort((a, b) => a.date.localeCompare(b.date));
  }

  const costByKey = {};
  const tokensByKey = {};

  for (const d of days) {
    if (!d.date) continue;
    const parts = d.date.split("-");
    if (parts.length < 3) continue;

    let key;
    if (granularity === "monthly") {
      // YYYY-MM
      key = `${parts[0]}-${parts[1]}`;
    } else if (granularity === "weekly") {
      // Regroupement par semaine (Lundi)
      const year = parseInt(parts[0], 10);
      const month = parseInt(parts[1], 10) - 1;
      const day = parseInt(parts[2], 10);
      const dt = new Date(Date.UTC(year, month, day));
      const dayOfWeek = dt.getUTCDay();
      const diffToMonday = (dayOfWeek + 6) % 7;
      const monday = new Date(dt.getTime() - diffToMonday * 86400000);
      key = monday.toISOString().slice(0, 10);
    }

    costByKey[key] = (costByKey[key] || 0) + (d.cost || 0);
    tokensByKey[key] = (tokensByKey[key] || 0) + (d.tokens || 0);
  }

  return Object.keys(costByKey).sort().map((k) => ({
    date: k,
    cost: costByKey[k],
    tokens: tokensByKey[k],
  }));
}

// ==========================================================================
// Tooltip au survol façon macOS : "Aug 17 – Aug 23 · $511.71"
// ==========================================================================
function spendBucketTooltip(dateKey, granularity, cost) {
  const months = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
  const parts = dateKey.split("-");
  const costStr = formatCurrency(cost);

  if (granularity === "monthly") {
    const m = parseInt(parts[1], 10) - 1;
    return `${months[m]} ${parts[0]} · ${costStr}`;
  }

  if (granularity === "weekly") {
    const startYear = parseInt(parts[0], 10);
    const startMonth = parseInt(parts[1], 10) - 1;
    const startDay = parseInt(parts[2], 10);
    const startDt = new Date(Date.UTC(startYear, startMonth, startDay));
    const endDt = new Date(startDt.getTime() + 6 * 86400000);
    const endMonth = endDt.getUTCMonth();
    const endDay = endDt.getUTCDate();

    const startStr = `${months[startMonth]} ${startDay}`;
    const endStr = `${months[endMonth]} ${endDay}`;
    return `${startStr} – ${endStr} · ${costStr}`;
  }

  // Daily : "Sep 14 · $42.10"
  if (parts.length >= 3) {
    const m = parseInt(parts[1], 10) - 1;
    const d = parseInt(parts[2], 10);
    return `${months[m]} ${d} · ${costStr}`;
  }

  return `${dateKey} · ${costStr}`;
}

// Formatage du label d'axe X selon la granularité (ex. "Sep 2025", "Jun 1", "14 Sep")
function formatAxisLabel(key, granularity) {
  const months = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
  const parts = key.split("-");
  if (granularity === "monthly") {
    const m = parseInt(parts[1], 10) - 1;
    return `${months[m]} ${parts[0]}`;
  }
  if (parts.length >= 3) {
    const m = parseInt(parts[1], 10) - 1;
    const d = parseInt(parts[2], 10);
    return `${months[m]} ${d}`;
  }
  return key;
}

// ==========================================================================
// Remplissage temporel continu pour les périodes (jours vides à $0 façon macOS)
// ==========================================================================
function fillContinuousDays(days, period) {
  if (!days || days.length === 0) return [];
  const map = new Map();
  days.forEach((d) => {
    if (d.date) map.set(d.date, d);
  });

  const now = new Date();
  const todayStr = now.toISOString().slice(0, 10);

  let startDateStr = null;
  let endDateStr = todayStr;

  if (period === "today") {
    startDateStr = todayStr;
  } else if (period === "yesterday") {
    const yest = new Date(now.getTime() - 86400000);
    startDateStr = yest.toISOString().slice(0, 10);
    endDateStr = startDateStr;
  } else if (period === "week") {
    const d = new Date(now.getTime() - 6 * 86400000);
    startDateStr = d.toISOString().slice(0, 10);
  } else if (period === "month") {
    const d = new Date(now.getTime() - 29 * 86400000);
    startDateStr = d.toISOString().slice(0, 10);
  } else if (period === "mtd") {
    const y = now.getUTCFullYear();
    const m = String(now.getUTCMonth() + 1).padStart(2, "0");
    startDateStr = `${y}-${m}-01`;
  } else {
    // all and reset-to-date use the first available source day.
    const sorted = [...days].filter((d) => d.date).sort((a, b) => a.date.localeCompare(b.date));
    if (sorted.length > 0) {
      startDateStr = sorted[0].date;
    } else {
      startDateStr = todayStr;
    }
  }

  const explicitStart = timelineStartDate(period, now);
  if (explicitStart) startDateStr = explicitStart;

  const startDt = new Date(startDateStr + "T00:00:00Z");
  const endDt = new Date(endDateStr + "T00:00:00Z");
  const result = [];
  let cur = new Date(startDt.getTime());

  while (cur <= endDt) {
    const curStr = cur.toISOString().slice(0, 10);
    if (map.has(curStr)) {
      result.push(map.get(curStr));
    } else {
      result.push({ date: curStr, cost: 0, tokens: 0 });
    }
    cur = new Date(cur.getTime() + 86400000);
  }

  return result;
}

// ==========================================================================
// Calcul robuste de l'axe Y (Zéro doublon, graduations claires même à $1 max)
// ==========================================================================
function computeYAxis(rawMax) {
  const safeMax = Math.max(rawMax, 0.001);
  let maxSpend;
  let isCurrencyDecimals = false;

  if (safeMax <= 0.1) {
    maxSpend = 0.1;
    isCurrencyDecimals = true;
  } else if (safeMax <= 0.5) {
    maxSpend = 0.5;
    isCurrencyDecimals = true;
  } else if (safeMax <= 1.0) {
    maxSpend = 1.0;
    isCurrencyDecimals = true;
  } else if (safeMax <= 2.0) {
    maxSpend = 2.0;
    isCurrencyDecimals = true;
  } else if (safeMax <= 3.0) {
    maxSpend = 3.0;
    isCurrencyDecimals = true;
  } else if (safeMax <= 10.0) {
    // Forcer un plafond pair pour que max / 2 soit un entier distinct (4 -> 2, 6 -> 3, 8 -> 4, 10 -> 5)
    maxSpend = Math.ceil(safeMax / 2) * 2;
    if (maxSpend < 4) maxSpend = 4;
  } else if (safeMax <= 50.0) {
    maxSpend = Math.ceil(safeMax / 10) * 10;
  } else if (safeMax <= 200.0) {
    maxSpend = Math.ceil(safeMax / 20) * 20;
  } else {
    const magnitude = Math.pow(10, Math.floor(Math.log10(safeMax)));
    maxSpend = Math.ceil(safeMax / magnitude) * magnitude;
    if ((maxSpend / (magnitude / 2)) % 2 !== 0) {
      maxSpend += magnitude / 2;
    }
  }

  let topLabel, midLabel, bottomLabel;
  if (isCurrencyDecimals) {
    topLabel = "$" + maxSpend.toFixed(2);
    midLabel = "$" + (maxSpend / 2).toFixed(2);
    bottomLabel = "$0";
  } else {
    topLabel = Math.round(maxSpend).toLocaleString();
    midLabel = Math.round(maxSpend / 2).toLocaleString();
    bottomLabel = "0";
  }

  return { maxSpend, topLabel, midLabel, bottomLabel };
}

// ==========================================================================
// Gestionnaire intelligent des boutons de granularité (Daily | Weekly | Monthly)
// Masquage automatique selon la timeline (pas de monthly sur 30j/7j/today)
// ==========================================================================
function setupGranularityPicker(pickerId, svgId, titleId, days, singleColor) {
  const picker = document.getElementById(pickerId);
  if (!picker) return;

  const isOneDay = ["today", "yesterday"].includes(currentPeriod);
  const isSevenDays = currentPeriod === "week";
  const isMonthOrLess = ["week", "wtd", "month", "mtd"].includes(currentPeriod);

  const dailyBtn = picker.querySelector('[data-gran="daily"]');
  const weeklyBtn = picker.querySelector('[data-gran="weekly"]');
  const monthlyBtn = picker.querySelector('[data-gran="monthly"]');

  if (dailyBtn) dailyBtn.style.display = "inline-flex";

  if (isOneDay) {
    if (weeklyBtn) weeklyBtn.style.display = "none";
    if (monthlyBtn) monthlyBtn.style.display = "none";
    currentSpendGranularity = "daily";
  } else if (isSevenDays) {
    if (weeklyBtn) weeklyBtn.style.display = "inline-flex";
    if (monthlyBtn) monthlyBtn.style.display = "none";
    if (currentSpendGranularity === "monthly") currentSpendGranularity = "daily";
  } else if (isMonthOrLess) {
    if (weeklyBtn) weeklyBtn.style.display = "inline-flex";
    if (monthlyBtn) monthlyBtn.style.display = "none";
    if (currentSpendGranularity === "monthly") currentSpendGranularity = "weekly";
  } else {
    if (weeklyBtn) weeklyBtn.style.display = "inline-flex";
    if (monthlyBtn) monthlyBtn.style.display = "inline-flex";
  }

  picker.querySelectorAll(".gran-btn").forEach((btn) => {
    btn.classList.toggle("active", btn.dataset.gran === currentSpendGranularity);
    const newBtn = btn.cloneNode(true);
    btn.parentNode.replaceChild(newBtn, btn);

    newBtn.addEventListener("click", () => {
      picker.querySelectorAll(".gran-btn").forEach((b) => b.classList.remove("active"));
      newBtn.classList.add("active");
      currentSpendGranularity = newBtn.dataset.gran;
      renderActivityChart(svgId, titleId, days, singleColor);
    });
  });
}

// ==========================================================================
// Rendu SVG de l'histogramme d'activité (ActivityChart 100% réel)
// Correction géométrique des barres (ancrage au sol) et suppression des doublons Y
// ==========================================================================
function renderActivityChart(svgId, titleId, days, singleColor) {
  const container = document.getElementById(svgId);
  if (!container) return;

  const heading = document.getElementById(titleId);
  if (heading) {
    const gran = currentSpendGranularity;
    heading.textContent = gran === "daily" ? "Daily spend" : gran === "weekly" ? "Weekly spend" : "Monthly spend";
  }

  // Échelle temporelle continue avec comblement des jours à $0
  const continuousDays = fillContinuousDays(days, currentPeriod);

  // Vrai regroupement mathématique sans coupure
  const aggregated = bucketDailyUsage(continuousDays, currentSpendGranularity);

  const width = container.clientWidth || 550;
  const height = 180;
  const padL = 48;
  const padR = 20;
  const padT = 16;
  const padB = 24;

  const chartW = width - padL - padR;
  const chartH = height - padT - padB;
  const baselineY = padT + chartH;

  if (aggregated.length === 0 || aggregated.every((d) => (d.cost || 0) === 0)) {
    container.innerHTML = `<div style="display:flex;align-items:center;justify-content:center;height:${height}px;color:var(--text-muted);font-size:12px;">No activity logged for this period.</div>`;
    return;
  }

  // Échelle Y robuste sans doublon
  const rawMax = Math.max(...aggregated.map((d) => d.cost || 0), 0.001);
  const { maxSpend, topLabel, midLabel, bottomLabel } = computeYAxis(rawMax);

  const count = aggregated.length;
  const step = chartW / count;
  const barWidth = Math.max(2, Math.min(32, step - (count > 60 ? 0.5 : count > 20 ? 1.5 : 3)));

  let barsSvg = "";
  let xLabelsSvg = "";
  const labelInterval = Math.max(1, Math.floor(count / 5));

  const cardParent = container.closest(".activity-card") || container.closest(".groupbox");
  const currencyLabel = cardParent?.querySelector(".currency-label");

  aggregated.forEach((d, i) => {
    const cost = d.cost || 0;
    const barH = (cost / maxSpend) * chartH;
    // La barre monte TOUJOURS depuis la ligne de base vers le haut (aucun dépassement sous l'axe X)
    const effectiveBarH = cost > 0 ? Math.max(2, barH) : 0;
    const x = padL + i * step + (step - barWidth) / 2;
    const y = baselineY - effectiveBarH;
    const color = singleColor || "#22c55e";

    barsSvg += `
      <rect class="activity-bar" data-index="${i}" x="${x}" y="${y}" width="${barWidth}" height="${effectiveBarH}" rx="2" fill="${color}" opacity="${cost > 0 ? '0.9' : '0.15'}" style="cursor: crosshair;">
      </rect>
    `;

    // Étiquettes régulières de l'axe X
    if (i % labelInterval === 0 || i === count - 1) {
      const labelText = formatAxisLabel(d.date, currentSpendGranularity);
      xLabelsSvg += `
        <text x="${x + barWidth / 2}" y="${padT + chartH + 16}" fill="#8e8e93" font-size="10" text-anchor="middle">${labelText}</text>
      `;
    }
  });

  // Lignes de repère Y et libellés sans doublon
  const yAxisLabels = `
    <text x="${padL - 8}" y="${padT + 8}" fill="#8e8e93" font-size="10" text-anchor="end">${topLabel}</text>
    <text x="${padL - 8}" y="${padT + chartH / 2 + 4}" fill="#8e8e93" font-size="10" text-anchor="end">${midLabel}</text>
    <text x="${padL - 8}" y="${baselineY}" fill="#8e8e93" font-size="10" text-anchor="end">${bottomLabel}</text>
  `;

  const yAxisSvg = `
    <line x1="${padL}" y1="${baselineY}" x2="${padL + chartW}" y2="${baselineY}" stroke="rgba(255,255,255,0.08)" stroke-width="1" />
    <line x1="${padL}" y1="${padT + chartH / 2}" x2="${padL + chartW}" y2="${padT + chartH / 2}" stroke="rgba(255,255,255,0.04)" stroke-dasharray="3,4" stroke-width="0.8" />
    <line x1="${padL}" y1="${padT}" x2="${padL + chartW}" y2="${padT}" stroke="rgba(255,255,255,0.04)" stroke-dasharray="3,4" stroke-width="0.8" />
    ${yAxisLabels}
  `;

  container.innerHTML = `
    <svg id="${svgId}-svg" width="100%" height="${height}" viewBox="0 0 ${width} ${height}" style="overflow: visible;">
      ${yAxisSvg}
      ${barsSvg}
      ${xLabelsSvg}
      <line id="${svgId}-hover-line" x1="0" y1="${padT}" x2="0" y2="${padT + chartH}" stroke="#3b82f6" stroke-width="1.5" opacity="0" pointer-events="none" />
    </svg>
  `;

  // Interactivité au survol de la souris : ligne verticale et tooltip en haut à droite
  const svgEl = document.getElementById(`${svgId}-svg`);
  const hoverLine = document.getElementById(`${svgId}-hover-line`);

  if (svgEl) {
    svgEl.addEventListener("mousemove", (e) => {
      const rect = svgEl.getBoundingClientRect();
      const mouseX = ((e.clientX - rect.left) / rect.width) * width;

      if (mouseX < padL || mouseX > padL + chartW) {
        if (hoverLine) hoverLine.setAttribute("opacity", "0");
        if (currencyLabel) {
          currencyLabel.textContent = "USD";
          currencyLabel.classList.remove("active-hover");
        }
        return;
      }

      const relX = mouseX - padL;
      const idx = Math.min(count - 1, Math.max(0, Math.floor(relX / step)));
      const target = aggregated[idx];

      if (target) {
        const barCenterX = padL + idx * step + step / 2;
        if (hoverLine) {
          hoverLine.setAttribute("x1", barCenterX);
          hoverLine.setAttribute("x2", barCenterX);
          hoverLine.setAttribute("opacity", "0.75");
        }
        if (currencyLabel) {
          currencyLabel.textContent = spendBucketTooltip(target.date, currentSpendGranularity, target.cost || 0);
          currencyLabel.classList.add("active-hover");
        }
      }
    });

    svgEl.addEventListener("mouseleave", () => {
      if (hoverLine) hoverLine.setAttribute("opacity", "0");
      if (currencyLabel) {
        currencyLabel.textContent = "USD";
        currencyLabel.classList.remove("active-hover");
      }
    });
  }
}


// État de tri par défaut (Spend descendant)
let modelsSortState = {
  column: "spend",
  direction: "desc"
};

// ==========================================================================
// Rendu du tableau des modèles avec tri interactif et filtre de recherche
// ==========================================================================
function renderModelsTable(models, totalCost, tbodyId, countId, searchInputId) {
  const tbody = document.getElementById(tbodyId);
  if (!tbody) return;

  const table = tbody.closest("table");

  if (countId) {
    const badge = document.getElementById(countId);
    if (badge) badge.textContent = models.length.toString();
  }

  function updateHeaders() {
    if (!table) return;
    table.querySelectorAll("th.sortable").forEach((th) => {
      const col = th.dataset.sort;
      const indicator = th.querySelector(".sort-indicator");
      if (indicator) {
        if (col === modelsSortState.column) {
          indicator.textContent = modelsSortState.direction === "desc" ? "↓" : "↑";
        } else {
          indicator.textContent = "";
        }
      }
    });
  }

  function updateRows(filterText = "") {
    tbody.innerHTML = "";
    const q = filterText.toLowerCase().trim();
    let filtered = models.filter((m) => !q || m.model.toLowerCase().includes(q));

    // Application du tri interactif
    filtered.sort((a, b) => {
      let res = 0;
      if (modelsSortState.column === "spend" || modelsSortState.column === "share") {
        res = (a.totalCost || 0) - (b.totalCost || 0);
      } else if (["input", "output", "cacheRead", "cacheWrite"].includes(modelsSortState.column)) {
        const fields = {
          input: "inputTokens",
          output: "outputTokens",
          cacheRead: "cacheReadTokens",
          cacheWrite: "cacheWriteTokens",
        };
        const field = fields[modelsSortState.column];
        res = (a[field] || 0) - (b[field] || 0);
      } else if (modelsSortState.column === "model") {
        res = (a.model || "").localeCompare(b.model || "");
      }
      return modelsSortState.direction === "desc" ? -res : res;
    });

    if (filtered.length === 0) {
      tbody.innerHTML = `<tr><td colspan="7" style="text-align:center;color:var(--text-muted);padding:18px;">No models match your filter.</td></tr>`;
      return;
    }

    filtered.forEach((m) => {
      const share = totalCost > 0 ? ((m.totalCost || 0) / totalCost) * 100 : m.percentage || 0;
      const pricingTitle = modelPricingTooltip(m.pricing);
      const pricingRows = modelPricingRows(m.pricing);
      const pricingContent = pricingRows.length > 0
        ? pricingRows.map(([label, value]) => `
            <div class="model-price-row">
              <span>${escapeHtml(label)}</span>
              <strong>${escapeHtml(value)}<small>/1M</small></strong>
            </div>
          `).join("")
        : `<div class="model-price-unavailable">Pricing unavailable</div>`;
      const tr = document.createElement("tr");
      tr.innerHTML = `
        <td class="col-model"><span>${escapeHtml(m.model)}</span><span class="model-price-wrap"><button class="model-price-info" type="button" aria-label="${escapeHtml(pricingTitle)}"><svg viewBox="0 0 20 20" aria-hidden="true"><circle cx="10" cy="10" r="7.25"></circle><path d="M12.4 7.4c-.55-.42-1.33-.68-2.2-.68-1.2 0-2.05.52-2.05 1.3 0 .83.7 1.12 2.02 1.38 1.75.34 2.75.92 2.75 2.25 0 1.39-1.17 2.35-2.92 2.35-.98 0-1.94-.29-2.65-.82M10 5.55v8.9"></path></svg></button><span class="model-price-popover" role="tooltip"><span class="model-price-heading">API pricing</span><span class="model-price-unit">USD per 1M tokens</span>${pricingContent}</span></span></td>
        <td class="col-token-part text-right">${formatCompactTokens(m.inputTokens || 0)}</td>
        <td class="col-token-part text-right">${formatCompactTokens(m.cacheReadTokens || 0)}</td>
        <td class="col-token-part text-right">${formatCompactTokens(m.cacheWriteTokens || 0)}</td>
        <td class="col-token-part text-right">${formatCompactTokens(m.outputTokens || 0)}</td>
        <td class="col-spend text-right">${formatCurrency(m.totalCost)}</td>
        <td class="col-share text-right">${share.toFixed(1)}%</td>
      `;
      tbody.appendChild(tr);
    });

    updateHeaders();
  }

  // Écouteurs de clic sur les en-têtes de colonnes
  if (table && !table.dataset.sortInitialized) {
    table.dataset.sortInitialized = "true";
    table._modelsSortCallback = createReplaceableCallback(updateRows);
    table.querySelectorAll("th.sortable").forEach((th) => {
      th.addEventListener("click", () => {
        const col = th.dataset.sort;
        if (modelsSortState.column === col) {
          modelsSortState.direction = modelsSortState.direction === "desc" ? "asc" : "desc";
        } else {
          modelsSortState.column = col;
          modelsSortState.direction = col === "model" ? "asc" : "desc";
        }
        const currentInput = document.getElementById(searchInputId);
        table._modelsSortCallback.run(currentInput ? currentInput.value : "");
      });
    });
  } else if (table) {
    table._modelsSortCallback.replace(updateRows);
  }

  updateRows();

  const input = document.getElementById(searchInputId);
  if (input) {
    input.value = "";
    input.oninput = (e) => updateRows(e.target.value);
  }
}

// ==========================================================================
// Rendu des abonnements
// ==========================================================================
function renderSubscriptions() {
  const card = document.getElementById("summary-subscriptions-card");
  const list = document.getElementById("summary-subscriptions-list");
  if (!card || !list) return;

  const subs = reportData?.subscription?.agents || [];
  if (subs.length === 0) {
    card.style.display = "none";
    return;
  }

  card.style.display = "block";
  list.innerHTML = "";

  subs.forEach((sub) => {
    const presentation = subscriptionPresentation(sub);
    const div = document.createElement("div");
    div.style.display = "flex";
    div.style.justifyContent = "space-between";
    div.style.fontSize = "12px";
    div.style.padding = "6px 0";
    div.innerHTML = `
      <span><strong>${escapeHtml(getAgentDisplayName(sub.agent))}</strong> · ${escapeHtml(presentation.plan)}</span>
      <span style="font-weight:600;">${presentation.monthlyPrice === null ? "Monthly price unavailable" : formatCurrency(presentation.monthlyPrice) + " / mo"}</span>
    `;
    list.appendChild(div);
  });
}

// ==========================================================================
// Paramètres (Settings)
// ==========================================================================
function initSettings() {
  document.getElementById("settings-btn")?.addEventListener("click", () => {
    switchTab("settings");
  });

  document.getElementById("settings-open-folder-btn")?.addEventListener("click", async () => {
    try {
      await invokeTauri("open_data_folder");
    } catch (e) {
      console.error(e);
    }
  });

  // Autostart
  invokeTauri("get_autostart_status")
    .then((enabled) => {
      const toggle = document.getElementById("settings-autostart-toggle");
      if (toggle) {
        toggle.checked = !!enabled;
        toggle.onchange = async () => {
          await invokeTauri("set_autostart", { enabled: toggle.checked });
        };
      }
    })
    .catch((error) => showStatusError(`Autostart status unavailable: ${error}`));

  invokeTauri("get_cli_status")
    .then((status) => {
      const badge = document.getElementById("settings-cli-status");
      const path = document.getElementById("settings-cli-path");
      if (badge) {
        badge.textContent = status.available ? "Operational" : "Unavailable";
        badge.className = status.available ? "status-badge-ok" : "status-badge-error";
      }
      if (path) path.textContent = status.path || "No agent-burn executable found";
    })
    .catch((error) => showStatusError(`CLI status unavailable: ${error}`));

  settingsLoadPromise = invokeTauri("get_settings")
    .then((settings) => {
      appSettings = settings;
      applySettingsToControls();
      return settings;
    })
    .catch((error) => {
      showStatusError(`Settings unavailable: ${error}`);
      return null;
    });

  for (const id of [
    "settings-cli-input",
    "settings-codex-homes-input",
    "settings-offline-toggle",
    "settings-refresh-select",
    "settings-antigravity-ultra-price",
  ]) {
    document.getElementById(id)?.addEventListener("change", saveSettingsFromControls);
  }

  // Fermeture du menu quota au clic externe
  document.addEventListener("click", () => {
    document.getElementById("quota-menu")?.classList.remove("open");
  });
}

function applySettingsToControls() {
  if (!appSettings) return;
  const cli = document.getElementById("settings-cli-input");
  const homes = document.getElementById("settings-codex-homes-input");
  const offline = document.getElementById("settings-offline-toggle");
  const refresh = document.getElementById("settings-refresh-select");
  const ultraPrice = document.getElementById("settings-antigravity-ultra-price");
  if (cli) cli.value = appSettings.customCliPath || "";
  if (homes) homes.value = appSettings.codexHomes || "";
  if (offline) offline.checked = !!appSettings.offline;
  if (refresh) refresh.value = String(appSettings.refreshMinutes || 1);
  if (ultraPrice) ultraPrice.value = appSettings.antigravityUltraPrice ? String(appSettings.antigravityUltraPrice) : "";
}

async function saveSettingsFromControls() {
  const settings = {
    customCliPath: document.getElementById("settings-cli-input")?.value.trim() || null,
    codexHomes: document.getElementById("settings-codex-homes-input")?.value.trim() || "",
    offline: !!document.getElementById("settings-offline-toggle")?.checked,
    refreshMinutes: Number(document.getElementById("settings-refresh-select")?.value || 1),
    quotaSource: appSettings?.quotaSource || "antigravity",
    antigravityUltraPrice: Number(document.getElementById("settings-antigravity-ultra-price")?.value) || null,
    hiddenAgents: appSettings?.hiddenAgents || [],
  };
  appSettings = await invokeTauri("set_settings", { settings });
  for (const key in harnessCache) delete harnessCache[key];
  await loadData(true);
  void preloadTimelines();
}

function renderSettings() {
  const ultraRow = document.getElementById("settings-antigravity-ultra-row");
  const antigravityPlan = (reportData?.subscription?.agents || []).find(
    (agent) => agent.agent?.toLowerCase() === "antigravity"
  );
  if (ultraRow) {
    const show = shouldShowAntigravityUltraSetting(antigravityPlan);
    ultraRow.hidden = !show;
    ultraRow.style.display = show ? "" : "none";
  }

  // 2. Liste des harnais détectés (aucun harnais non détecté !)
  const list = document.getElementById("settings-detected-agents-list");
  if (list) {
    list.innerHTML = "";
    if (detectedAgents.length === 0) {
      list.innerHTML = `<div style="color:var(--text-muted);font-size:12px;">No active agents detected yet.</div>`;
    } else {
      detectedAgents.forEach((agent) => {
        const row = document.createElement("div");
        row.className = "detected-agent-row";
        row.innerHTML = `
          <div style="display:flex;align-items:center;gap:10px;">
            <img src="${getAgentBrandIcon(agent)}" width="22" height="22" style="border-radius:4px;" />
            <strong>${escapeHtml(getAgentDisplayName(agent))}</strong>
          </div>
          <label class="switch" title="Show ${escapeHtml(getAgentDisplayName(agent))} in the tab bar">
            <input type="checkbox" data-agent-tab-toggle="${escapeHtml(agent)}" ${appSettings?.hiddenAgents?.includes(agent) ? "" : "checked"} />
            <span class="slider"></span>
          </label>
        `;
        list.appendChild(row);
      });
      list.querySelectorAll("[data-agent-tab-toggle]").forEach((toggle) => {
        toggle.addEventListener("change", async () => {
          const agent = toggle.dataset.agentTabToggle;
          const hidden = new Set(appSettings?.hiddenAgents || []);
          if (toggle.checked) hidden.delete(agent);
          else hidden.add(agent);
          appSettings = await invokeTauri("set_settings", {
            settings: { ...appSettings, hiddenAgents: [...hidden] },
          });
          if (!toggle.checked && currentTab === agent) switchTab("summary");
          else renderHarnessTabs();
        });
      });
    }
  }
}

// ==========================================================================
// Bouton de rafraîchissement
// ==========================================================================
function initRefresh() {
  document.getElementById("refresh-btn")?.addEventListener("click", async () => {
    for (const k in harnessCache) delete harnessCache[k];
    await refreshAllTimelineCaches(true);
  });
}

// ==========================================================================
// Timer de pied de page ("Updated 11 sec ago")
// ==========================================================================
function initFooterTimer() {
  const el = document.getElementById("footer-status-text");
  if (!el) return;

  setInterval(() => {
    const visibleUpdatedAt = periodCache[timelineCacheKey(currentPeriod)]?.updatedAt;
    if (!Number.isFinite(visibleUpdatedAt)) {
      el.textContent = "Waiting for first update";
      return;
    }
    const sec = Math.max(0, Math.floor((Date.now() - visibleUpdatedAt) / 1000));
    const staleAfter = (appSettings?.refreshMinutes || 1) * 60 + 30;
    const dot = document.querySelector("#quota-menu-pill .quota-dot");
    if (sec > staleAfter && dot) {
      dot.className = "quota-dot stale";
    }
    if (sec < 5) {
      el.textContent = "Updated just now";
    } else if (sec < 60) {
      el.textContent = `Updated ${sec} sec ago`;
    } else {
      const min = Math.floor(sec / 60);
      el.textContent = `Updated ${min} min ago`;
    }
  }, 1000);
}
