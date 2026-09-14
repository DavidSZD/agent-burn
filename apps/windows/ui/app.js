// Pont IPC direct Tauri v2 (aucune fausse donnée de test)
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

let currentPeriod = "mtd";
let currentTab = "summary";
let reportData = null;
let projectsData = null;
const periodCache = {};
const harnessCache = {};
const projectsCache = {};
let debouncePeriodTimer = null;

function getPeriodLabel(p) {
  switch (p) {
    case "today":
      return "Aujourd'hui";
    case "week":
      return "7 derniers jours";
    case "mtd":
      return "Ce mois-ci";
    case "all":
      return "Tout l'historique";
    default:
      return p || "Période";
  }
}


// Initialisation
window.addEventListener("DOMContentLoaded", async () => {
  initTabs();
  initPeriods();
  initRefresh();
  initSearch();
  initCustomization();
  initTauriEvents();
  initAutostart();
  initDataFolder();


  // Démarrage instantané (0 ms) via le cache disque report-cache.json
  await initColdStart();

  await checkCliStatus();
  await loadData();
});

async function initColdStart() {
  try {
    const cached = await invokeTauri("get_report_cache");
    if (cached && (cached.summary || cached.projects || cached.antigravity)) {
      if (cached.antigravity) {
        antigravityData = cached.antigravity;
        antigravityCache[currentPeriod] = cached.antigravity;
      }
      if (cached.summary) {
        reportData = cached.summary;
        periodCache[currentPeriod] = cached.summary;
        renderSummary();
      }
      if (cached.projects) {
        projectsData = cached.projects;
        projectsCache[currentPeriod] = cached.projects;
        renderProjectsView(cached.projects);
      }
      const statusBar = document.getElementById("status-text");
      if (statusBar) statusBar.textContent = "⚡ Prêt immédiatement (actualisation en fond...)";
    }
  } catch (e) {
    console.debug("Aucun cache disque initial disponible", e);
  }
}

function initTabs() {
  document.querySelectorAll(".tab-btn").forEach((btn) => {
    btn.addEventListener("click", async () => {
      document.querySelectorAll(".tab-btn").forEach((b) => b.classList.remove("active"));
      document.querySelectorAll(".view-panel").forEach((p) => p.classList.remove("active"));

      btn.classList.add("active");
      currentTab = btn.dataset.tab;

      // S'assurer que les boutons de timeline reflètent toujours la période active
      document.querySelectorAll(".period-btn").forEach((b) => {
        b.classList.toggle("active", b.dataset.period === currentPeriod);
      });

      if (currentTab === "summary") {
        document.getElementById("view-summary").classList.add("active");
        if (fullReportData) {
          reportData = sliceSummaryData(fullReportData, currentPeriod);
          periodCache[currentPeriod] = reportData;
        }
        if (reportData) {
          renderSummary();
        } else {
          await loadData();
        }
      } else if (currentTab === "projects") {
        document.getElementById("view-projects").classList.add("active");
        await loadProjects();
      } else if (currentTab === "antigravity") {
        document.getElementById("view-antigravity").classList.add("active");
        await loadAntigravity();
      } else if (currentTab === "settings") {
        document.getElementById("view-settings").classList.add("active");
      } else {
        document.getElementById("view-harness").classList.add("active");
        renderHarnessView(currentTab);
      }
    });
  });
}

function initPeriods() {
  document.querySelectorAll(".period-btn").forEach((btn) => {
    btn.addEventListener("click", () => {
      document.querySelectorAll(".period-btn").forEach((b) => b.classList.remove("active"));
      btn.classList.add("active");
      currentPeriod = btn.dataset.period;

      // 1. Découpage instantané en 0 ms si données en mémoire
      if (fullReportData) {
        reportData = sliceSummaryData(fullReportData, currentPeriod);
        periodCache[currentPeriod] = reportData;
      }

      // 2. Mise à jour immédiate selon l'onglet courant
      if (currentTab === "summary") {
        if (reportData) renderSummary();
        updateStatusSynced();
      } else if (currentTab === "antigravity") {
        if (antigravityCache[currentPeriod]) {
          antigravityData = antigravityCache[currentPeriod];
          renderAntigravityView(antigravityData);
          updateStatusSynced();
        }
      } else if (currentTab === "projects") {
        if (projectsCache[currentPeriod]) {
          projectsData = projectsCache[currentPeriod];
          renderProjectsView(projectsData);
          updateStatusSynced();
        }
      } else if (currentTab !== "settings") {
        renderHarnessView(currentTab);
      }

      // Anti-rebond (Debounce 50 ms) pour requêtes complémentaires
      clearTimeout(debouncePeriodTimer);
      debouncePeriodTimer = setTimeout(async () => {
        if (currentTab === "projects") {
          await loadProjects();
        } else if (currentTab === "antigravity") {
          await loadAntigravity();
        } else if (currentTab === "summary") {
          await loadData();
        } else if (currentTab !== "settings") {
          renderHarnessView(currentTab);
        }
      }, 50);
    });
  });
}


function initRefresh() {
  document.getElementById("refresh-btn")?.addEventListener("click", async () => {
    for (const k in periodCache) delete periodCache[k];
    for (const k in harnessCache) delete harnessCache[k];
    for (const k in projectsCache) delete projectsCache[k];
    for (const k in antigravityCache) delete antigravityCache[k];
    
    if (currentTab === "projects") {
      await loadProjects(true);
    } else if (currentTab === "antigravity") {
      await loadAntigravity(true);
    } else {
      await loadData(true);
      if (currentTab !== "summary" && currentTab !== "settings") {
        renderHarnessView(currentTab);
      }
    }
  });
}

function initSearch() {
  // Recherche en direct dans les projets
  document.getElementById("projects-search")?.addEventListener("input", (e) => {
    const q = e.target.value.toLowerCase().trim();
    filterTableRows("#projects-tbody", q, 7, "Aucun projet ne correspond à votre recherche");
  });

  // Recherche en direct dans les modèles
  document.getElementById("models-search")?.addEventListener("input", (e) => {
    const q = e.target.value.toLowerCase().trim();
    filterTableRows("#models-tbody", q, 3, "Aucun modèle ne correspond");
  });
}

function filterTableRows(tbodySelector, query, colSpan, emptyMsg) {
  const tbody = document.querySelector(tbodySelector);
  if (!tbody) return;
  const rows = tbody.querySelectorAll("tr");
  let visible = 0;

  rows.forEach((tr) => {
    if (tr.classList.contains("no-match-row")) return;
    const txt = tr.textContent.toLowerCase();
    const match = !query || txt.includes(query);
    tr.style.display = match ? "" : "none";
    if (match) visible++;
  });

  let noMatch = tbody.querySelector(".no-match-row");
  if (visible === 0 && rows.length > 0) {
    if (!noMatch) {
      noMatch = document.createElement("tr");
      noMatch.className = "no-match-row";
      noMatch.innerHTML = `<td colspan="${colSpan}" style="text-align: center; color: var(--text-muted); padding: 20px;">${emptyMsg}</td>`;
      tbody.appendChild(noMatch);
    }
  } else if (noMatch) {
    noMatch.remove();
  }
}


function initTauriEvents() {
  if (window.__TAURI__?.event) {
    window.__TAURI__.event.listen("quotas_updated", (event) => {
      console.log("Mise à jour en tâche de fond reçue", event.payload);
      if (event.payload) {
        reportData = event.payload;
        renderSummary();
      }
    });

    window.__TAURI__.event.listen("refresh_requested", () => {
      loadData();
    });
  }
}

async function checkCliStatus() {
  try {
    const status = await invokeTauri("get_cli_status");
    const ind = document.getElementById("cli-status-indicator");
    const path = document.getElementById("cli-path-display");
    const dirEl = document.getElementById("data-dir-display");
    if (status?.available) {
      ind.textContent = "Opérationnel (Binaire natif)";
      ind.classList.add("active");
      path.textContent = status.path || "agent-burn.exe";
    } else {
      ind.textContent = "Mode NPX automatique";
      path.textContent = "npx agent-burn@latest";
    }
    if (status?.dataDir && dirEl) {
      dirEl.textContent = status.dataDir;
    }
  } catch (err) {
    console.error("Erreur détection CLI:", err);
  }
}

let currentSummaryRequestId = 0;
let currentProjectsRequestId = 0;
let warmupDone = false;

function renderCurrentTabContent() {
  if (currentTab === "summary") {
    renderSummary();
  } else if (currentTab === "projects") {
    renderProjectsView(projectsData);
  } else if (currentTab === "antigravity") {
    renderAntigravityView(antigravityData);
  } else if (currentTab !== "settings") {
    renderHarnessView(currentTab);
  }
}

function updateStatusSynced() {
  const statusBar = document.getElementById("status-text");
  const statusDot = document.querySelector(".status-dot");
  if (statusBar) {
    const now = new Date().toLocaleTimeString();
    statusBar.textContent = `Données réelles synchronisées (${now})`;
    if (statusDot) statusDot.style.background = "var(--accent-green)";
  }
}

let fullReportData = null;

function sliceSummaryData(fullData, period) {
  if (!fullData || !fullData.daily) return fullData;
  if (period === "all") return fullData;

  const now = new Date();
  const todayStr = now.toISOString().split("T")[0];

  const sevenDaysAgo = new Date(now.getTime() - 7 * 24 * 60 * 60 * 1000);
  const weekStr = sevenDaysAgo.toISOString().split("T")[0];

  const mtdStr = `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, "0")}-01`;

  const dateFilter = (dateStr) => {
    if (period === "today") return dateStr === todayStr;
    if (period === "week") return dateStr >= weekStr;
    if (period === "mtd") return dateStr >= mtdStr;
    return true;
  };

  const filteredDaily = (fullData.daily || []).filter((d) => dateFilter(d.date));
  const totalCost = filteredDaily.reduce((acc, d) => acc + (d.cost || 0), 0);
  const totalTokens = filteredDaily.reduce((acc, d) => acc + (d.tokens || 0), 0);

  const filteredAgents = (fullData.agents || []).map((ag) => {
    const agDaily = (ag.daily || []).filter((d) => dateFilter(d.date));
    const agCost = agDaily.reduce((acc, d) => acc + (d.cost || 0), 0);
    const agTokens = agDaily.reduce((acc, d) => acc + (d.tokens || 0), 0);
    return {
      ...ag,
      daily: agDaily,
      totalCost: agCost,
      totalTokens: agTokens,
    };
  });

  return {
    ...fullData,
    daily: filteredDaily,
    agents: filteredAgents,
    totals: {
      totalCost,
      totalTokens,
    },
  };
}

function autoDetectCursor(data) {
  const hasCursor =
    data?.subscription?.agents?.some((a) => a.agent?.toLowerCase() === "cursor") ||
    data?.models?.some((m) => m.model?.toLowerCase().includes("cursor")) ||
    data?.agents?.some((a) => a.agent === "cursor");
  if (hasCursor) {
    const saved = localStorage.getItem("agent_burn_visible_tabs");
    let visibleTabs = { summary: true, projects: true, antigravity: true, codex: true, claude: false, cursor: true };
    if (saved) {
      try {
        visibleTabs = Object.assign(visibleTabs, JSON.parse(saved));
        if (visibleTabs.cursor === undefined) {
          visibleTabs.cursor = true;
          localStorage.setItem("agent_burn_visible_tabs", JSON.stringify(visibleTabs));
        }
      } catch (_) {}
    } else {
      localStorage.setItem("agent_burn_visible_tabs", JSON.stringify(visibleTabs));
    }
    applyTabVisibility("cursor", visibleTabs.cursor !== false);
    const cb = document.querySelector(`input[data-tab-target="cursor"]`);
    if (cb) cb.checked = visibleTabs.cursor !== false;
  }
}

async function loadData(force = false) {
  const statusBar = document.getElementById("status-text");
  const reqId = ++currentSummaryRequestId;
  const targetPeriod = currentPeriod;

  // 1. Découpage instantané en 0 ms si données complètes en mémoire
  if (!force && fullReportData) {
    reportData = sliceSummaryData(fullReportData, targetPeriod);
    periodCache[targetPeriod] = reportData;
    renderCurrentTabContent();
    updateStatusSynced();
    return;
  }

  // 2. Si déjà en cache pour cette période précise
  if (!force && periodCache[targetPeriod]) {
    reportData = periodCache[targetPeriod];
    renderCurrentTabContent();
    updateStatusSynced();
    return;
  }

  if (statusBar) statusBar.textContent = `Calcul et analyse des logs (${getPeriodLabel(targetPeriod)})...`;

  try {
    const data = await invokeTauri("get_summary", { period: targetPeriod });

    if (targetPeriod === "all") {
      fullReportData = data;
    }

    reportData = data;
    periodCache[targetPeriod] = data;

    autoDetectCursor(reportData);

    // Charger ou synchroniser Antigravity en arrière-plan
    if (!antigravityCache[targetPeriod]) {
      invokeTauri("get_antigravity_summary", { period: targetPeriod }).then((agd) => {
        antigravityCache[targetPeriod] = agd;
        if (currentTab === "summary") renderSummary();
      }).catch(() => {});
    }

    // Garde-fou d'affichage : on ne modifie l'écran que si l'utilisateur est toujours sur cette timeline
    if (reqId === currentSummaryRequestId && targetPeriod === currentPeriod) {
      renderCurrentTabContent();
      updateStatusSynced();
      await persistCache();
    }

    // Lancer le pré-chargement en arrière-plan des autres périodes pour que tout soit instantané
    if (!warmupDone) {
      warmupDone = true;
      setTimeout(warmupAllPeriods, 1000);
    }
  } catch (error) {
    console.error("Échec chargement des données:", error);
    if (reqId === currentSummaryRequestId) {
      if (statusBar) statusBar.textContent = `Erreur: ${error.message || error}`;
      const statusDot = document.querySelector(".status-dot");
      if (statusDot) statusDot.style.background = "#ef4444";
    }
  }
}

async function warmupAllPeriods() {
  const periods = ["today", "week", "mtd", "all"];
  for (const p of periods) {
    if (!periodCache[p]) {
      try {
        const d = await invokeTauri("get_summary", { period: p });
        periodCache[p] = d;
      } catch (_) {}
    }
    if (!projectsCache[p]) {
      try {
        const pd = await invokeTauri("get_projects_usage", { period: p });
        projectsCache[p] = pd;
      } catch (_) {}
    }
  }
  if (!harnessCache["codex"]) {
    try {
      harnessCache["codex"] = await invokeTauri("get_harness", { agent: "codex" });
    } catch (_) {}
  }
  await persistCache();
  console.log("⚡ Pré-chargement de toutes les timelines terminé en mémoire !");
}

async function persistCache() {
  try {
    await invokeTauri("save_report_cache", {
      data: {
        summary: reportData,
        projects: projectsCache[currentPeriod] || projectsData,
        antigravity: antigravityCache[currentPeriod] || antigravityData,
        updatedAt: new Date().toISOString(),
      },
    });
  } catch (e) {
    console.debug("Persistance cache disque ignorée", e);
  }
}

async function loadProjects(force = false) {
  const statusBar = document.getElementById("status-text");
  const reqId = ++currentProjectsRequestId;
  const targetPeriod = currentPeriod;

  if (!force && projectsCache[targetPeriod]) {
    projectsData = projectsCache[targetPeriod];
    renderProjectsView(projectsData);
    updateStatusSynced();
    return;
  }

  if (statusBar) statusBar.textContent = `Analyse des projets locaux (${getPeriodLabel(targetPeriod)})...`;

  try {
    const data = await invokeTauri("get_projects_usage", { period: targetPeriod });

    // TOUJOURS sauvegarder en mémoire
    projectsCache[targetPeriod] = data;

    // Garde-fou d'affichage
    if (reqId === currentProjectsRequestId && targetPeriod === currentPeriod) {
      projectsData = data;
      renderProjectsView(data);
      updateStatusSynced();
      await persistCache();
    }
  } catch (err) {
    console.error("Échec analyse projets:", err);
    if (reqId === currentProjectsRequestId) {
      if (statusBar) statusBar.textContent = `Erreur analyse projets: ${err}`;
      const statusDot = document.querySelector(".status-dot");
      if (statusDot) statusDot.style.background = "#ef4444";
    }
  }
}


function renderProjectsView(data) {
  if (!data) return;

  const countEl = document.getElementById("projects-count");
  const topNameEl = document.getElementById("projects-top-name");
  const topSubEl = document.getElementById("projects-top-sub");
  const totalCostEl = document.getElementById("projects-total-cost");
  const totalTokensEl = document.getElementById("projects-total-tokens");
  const tbody = document.getElementById("projects-tbody");

  if (countEl) countEl.textContent = data.totalProjects || 0;
  if (totalCostEl) totalCostEl.textContent = formatCurrency(data.totalCost || 0);
  if (totalTokensEl) totalTokensEl.textContent = `${formatNumber(data.totalTokens || 0)} tokens`;

  const projects = data.projects || [];
  if (projects.length > 0) {
    if (topNameEl) topNameEl.textContent = projects[0].name;
    if (topSubEl) topSubEl.textContent = `${formatNumber(projects[0].total_tokens)} tokens (${formatCurrency(projects[0].estimated_cost)})`;
  } else {
    if (topNameEl) topNameEl.textContent = "Aucun";
    if (topSubEl) topSubEl.textContent = "0 token sur cette période";
  }

  if (!tbody) return;
  tbody.innerHTML = "";

  if (projects.length === 0) {
    tbody.innerHTML = `
      <tr class="no-match-row">
        <td colspan="7" style="padding: 24px; text-align: center; color: var(--text-muted);">
          Aucune session détectée pour les projets locaux sur la période sélectionnée (${currentPeriod}).
        </td>
      </tr>
    `;
    return;
  }

  projects.forEach((proj) => {
    const tr = document.createElement("tr");

    let formattedDate = "-";
    if (proj.last_active) {
      try {
        const d = new Date(proj.last_active);
        formattedDate = d.toLocaleDateString("fr-FR", {
          day: "2-digit",
          month: "2-digit",
          year: "numeric",
          hour: "2-digit",
          minute: "2-digit",
        });
      } catch (_) {
        formattedDate = proj.last_active;
      }
    }

    const fillWidth = Math.max(3, Math.min(100, proj.relative_percent || 0));

    tr.innerHTML = `
      <td>
        <div class="project-name-cell">
          <strong>${escapeHtml(proj.name)}</strong>
        </div>
      </td>
      <td>
        <div class="project-path-cell" title="${escapeHtml(proj.path)}">
          ${escapeHtml(proj.path)}
        </div>
      </td>
      <td>
        <span class="project-sessions-badge">${proj.session_count} session${proj.session_count > 1 ? "s" : ""}</span>
      </td>
      <td style="color: var(--text-muted); font-size: 11px;">
        ${formattedDate}
      </td>
      <td>
        <div class="token-progress-wrapper">
          <span class="token-val">${formatNumber(proj.total_tokens)}</span>
          <div class="token-progress-bar">
            <div class="token-progress-fill" style="width: ${fillWidth}%;"></div>
          </div>
        </div>
      </td>
      <td>
        <strong style="color: var(--accent-flame);">${formatCurrency(proj.estimated_cost)}</strong>
      </td>
      <td>
        <button class="btn-open-project" data-path="${escapeHtml(proj.path)}" title="Ouvrir dans l'Explorateur Windows">
          📂 Ouvrir
        </button>
      </td>
    `;

    tbody.appendChild(tr);
  });

  // Brancher les boutons d'ouverture de dossier Windows Explorer
  tbody.querySelectorAll(".btn-open-project").forEach((btn) => {
    btn.addEventListener("click", async (e) => {
      e.stopPropagation();
      const path = btn.getAttribute("data-path");
      try {
        await invokeTauri("open_project_folder", { path });
      } catch (err) {
        alert("Impossible d'ouvrir le dossier: " + err);
      }
    });
  });

  // Réappliquer la recherche si une requête est en cours
  const searchInput = document.getElementById("projects-search");
  if (searchInput && searchInput.value.trim()) {
    filterTableRows("#projects-tbody", searchInput.value.toLowerCase().trim(), 7, "Aucun projet ne correspond");
  }
}

let antigravityData = null;
const antigravityCache = {};
let currentAntigravityRequestId = 0;

async function loadAntigravity(force = false) {
  const container = document.getElementById("antigravity-content");
  if (!container) return;

  const targetPeriod = currentPeriod;
  const reqId = ++currentAntigravityRequestId;

  if (!force && antigravityCache[targetPeriod]) {
    antigravityData = antigravityCache[targetPeriod];
    renderAntigravityView(antigravityData);
    updateStatusSynced();
    return;
  }

  container.innerHTML = `<div style="padding: 40px; text-align: center; color: var(--text-muted);">
    <div>Analyse des 56 sessions Google Antigravity (${getPeriodLabel(targetPeriod)})...</div>
  </div>`;

  try {
    const data = await invokeTauri("get_antigravity_summary", { period: targetPeriod });
    antigravityCache[targetPeriod] = data;

    if (reqId === currentAntigravityRequestId && targetPeriod === currentPeriod) {
      antigravityData = data;
      renderAntigravityView(data);
      updateStatusSynced();
    }
  } catch (err) {
    console.error("Échec chargement Antigravity:", err);
    if (reqId === currentAntigravityRequestId) {
      container.innerHTML = `
        <div class="card settings-card">
          <h3 style="color: #ef4444;">Erreur de chargement Antigravity</h3>
          <p style="color: var(--text-muted); margin-top: 8px;">${escapeHtml(err.message || err)}</p>
        </div>
      `;
    }
  }
}

function renderAntigravityView(data) {
  const container = document.getElementById("antigravity-content");
  if (!container) return;

  if (!data || data.session_count === 0) {
    container.innerHTML = `
      <div class="card settings-card">
        <div style="display: flex; justify-content: space-between; align-items: center;">
          <h3 style="color: var(--accent-flame); font-size: 18px;">GOOGLE ANTIGRAVITY</h3>
          <span class="badge">0 session</span>
        </div>
        <p style="color: var(--text-muted); margin-top: 12px;">
          Aucune session Antigravity enregistrée pour la période sélectionnée (<strong>${getPeriodLabel(currentPeriod)}</strong>).
          Sélectionnez « Tout l'historique » ou « Ce mois-ci » pour explorer vos 56 sessions antérieures.
        </p>
      </div>
    `;
    return;
  }

  const topModel = data.top_models && data.top_models.length > 0 ? data.top_models[0].model : "gemini-2.5-pro";

  let modelsTableHtml = "";
  if (data.top_models && data.top_models.length > 0) {
    modelsTableHtml = `
      <div class="section-title" style="margin-top: 24px;">Modèles Google Gemini utilisés (${getPeriodLabel(currentPeriod)})</div>
      <div class="table-container">
        <table>
          <thead>
            <tr>
              <th>Modèle</th>
              <th>Tokens</th>
              <th>Coût équivalent API (Source officielle LiteLLM)</th>
            </tr>
          </thead>
          <tbody>
            ${data.top_models.map(m => `
              <tr>
                <td><strong>${escapeHtml(m.model)}</strong></td>
                <td>${formatNumber(m.tokens)}</td>
                <td><strong style="color: var(--accent-flame);">${formatCurrency(m.cost)}</strong></td>
              </tr>
            `).join("")}
          </tbody>
        </table>
      </div>
    `;
  }

  let sessionsTableHtml = "";
  if (data.sessions && data.sessions.length > 0) {
    sessionsTableHtml = `
      <div class="section-title" style="margin-top: 24px;">Historique détaillé des sessions Antigravity (${data.sessions.length} session${data.sessions.length > 1 ? "s" : ""})</div>
      <div class="table-container">
        <table>
          <thead>
            <tr>
              <th>Projet</th>
              <th>Répertoire</th>
              <th>Dernière activité</th>
              <th>Étapes</th>
              <th>Tokens totaux</th>
              <th>Coût équivalent API</th>
            </tr>
          </thead>
          <tbody>
            ${data.sessions.map(s => {
              let formattedDate = "-";
              if (s.date) {
                try {
                  const d = new Date(s.date);
                  formattedDate = d.toLocaleDateString("fr-FR", {
                    day: "2-digit",
                    month: "2-digit",
                    year: "numeric",
                    hour: "2-digit",
                    minute: "2-digit",
                  });
                } catch (_) {
                  formattedDate = s.date;
                }
              }
              return `
                <tr>
                  <td>
                    <div class="project-name-cell">
                      <strong>${escapeHtml(s.project_name)}</strong>
                    </div>
                  </td>
                  <td>
                    <div class="project-path-cell" title="${escapeHtml(s.project_path)}">
                      ${escapeHtml(s.project_path)}
                    </div>
                  </td>
                  <td style="color: var(--text-muted); font-size: 11px;">${formattedDate}</td>
                  <td><span class="project-sessions-badge">${s.steps_count} étapes</span></td>
                  <td><strong>${formatNumber(s.total_tokens)}</strong> <span style="font-size: 10px; color: var(--text-muted);">(${formatNumber(s.input_tokens)} in / ${formatNumber(s.output_tokens)} out)</span></td>
                  <td><strong style="color: var(--accent-flame);">${formatCurrency(s.cost)}</strong></td>
                </tr>
              `;
            }).join("")}
          </tbody>
        </table>
      </div>
    `;
  }

  container.innerHTML = `
    <div class="card settings-card">
      <div style="display: flex; justify-content: space-between; align-items: center;">
        <h3 style="color: var(--accent-flame); font-size: 18px;">GOOGLE ANTIGRAVITY</h3>
        <span class="badge active">${data.session_count} session${data.session_count > 1 ? "s" : ""}</span>
      </div>

      <div class="metrics-grid" style="margin-top: 16px;">
        <div class="metric-card highlight">
          <div class="metric-label">Valeur consommée API · ${getPeriodLabel(currentPeriod)}</div>
          <div class="metric-value">${formatCurrency(data.total_cost)}</div>
          <div class="metric-sub">Tarif officiel LiteLLM Vertex/AI Studio</div>
        </div>
        <div class="metric-card">
          <div class="metric-label">Tokens consommés · ${getPeriodLabel(currentPeriod)}</div>
          <div class="metric-value" style="color: var(--accent-flame);">${formatNumber(data.total_tokens)}</div>
          <div class="metric-sub">${formatNumber(data.input_tokens)} in · ${formatNumber(data.output_tokens)} out</div>
        </div>
        <div class="metric-card">
          <div class="metric-label">Modèle principal</div>
          <div class="metric-value" style="font-size: 18px; margin-top: 4px;">${escapeHtml(topModel)}</div>
          <div class="metric-sub">Antigravity Autonomous Coding Agent</div>
        </div>
      </div>
    </div>

    ${modelsTableHtml}
    ${sessionsTableHtml}
  `;
}


function escapeHtml(str) {
  if (!str) return "";
  return String(str)
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#039;");
}


function renderSummary() {
  if (!reportData) return;

  const agData = antigravityCache[currentPeriod] || antigravityData;
  const agCost = agData?.total_cost || 0;
  const agTokens = agData?.total_tokens || 0;

  // Métriques globales unifiées (Codex + Cursor + Antigravity)
  const baseCost = reportData.totals?.totalCost ?? reportData.totalCost ?? 0;
  const baseTokens = reportData.totals?.totalTokens ?? reportData.totalTokens ?? 0;
  const totalCost = baseCost + agCost;
  const totalTokens = baseTokens + agTokens;

  document.getElementById("metric-spend").textContent = formatCurrency(totalCost);
  document.getElementById("metric-tokens").textContent = formatNumber(totalTokens);

  const sub = reportData.subscription;
  const agentWithEstimate = (sub?.agents || []).find((a) => a.estimate?.valueMultiple != null);
  if (agentWithEstimate && agentWithEstimate.estimate) {
    document.getElementById("metric-value-mult").textContent = `${agentWithEstimate.estimate.valueMultiple.toFixed(1)}×`;
    document.getElementById("metric-plan-name").textContent = `${agentWithEstimate.agent.toUpperCase()} ${agentWithEstimate.plan || "Plus"} ($${agentWithEstimate.pricePerMonth || 20}/mo)`;
  } else if (sub?.valueMultiple) {
    document.getElementById("metric-value-mult").textContent = `${sub.valueMultiple.toFixed(1)}×`;
    document.getElementById("metric-plan-name").textContent = `${sub.planName || "Forfait actif"} ($${sub.monthlyPrice || 0}/mo)`;
  } else {
    document.getElementById("metric-value-mult").textContent = "-";
  }

  // Rendu du graphique d'évolution journalière
  renderDailyChart(reportData.daily || []);

  // Quotas par agent et statut direct
  const quotasContainer = document.getElementById("quotas-container");
  if (quotasContainer) {
    quotasContainer.innerHTML = "";
    const agents = sub?.agents || reportData.agents || [];
    if (agents.length === 0 && (!agData || agData.session_count === 0)) {
      quotasContainer.innerHTML = `<div class="quota-card" style="grid-column: 1/-1; color: var(--text-muted);">Aucun quota d'agent en cours détecté.</div>`;
    } else {
      agents.forEach((agent) => {
        let remaining = agent.window?.usedPercent != null ? (100 - agent.window.usedPercent) : (agent.quota?.remainingPercent ?? 100);
        let reset = agent.window?.elapsedMinutes != null ? `${(agent.window.elapsedMinutes / 60).toFixed(1)}h écoulées` : (agent.quota?.resetCountdown ?? "Dans le cycle");
        let plan = agent.plan || "Défaut";

        if (agent.agent === "cursor" && reportData.cursorAccount) {
          const ca = reportData.cursorAccount;
          if (ca.activePercentUsed != null) {
            remaining = Math.max(0, 100 - ca.activePercentUsed);
            reset = `${ca.activePercentUsed}% quota utilisé`;
            plan = "Usage direct";
          }
        }

        const costVal = agent.periodUsage ?? agent.periodCost ?? 0;
        const card = document.createElement("div");
        card.className = "quota-card";
        card.innerHTML = `
          <div class="quota-header">
            <span class="quota-title">${escapeHtml(agent.agent.toUpperCase())} (${escapeHtml(plan)})</span>
            <span class="quota-percent">${remaining.toFixed(0)}% restant</span>
          </div>
          <div class="progress-bar-bg">
            <div class="progress-bar-fill" style="width: ${remaining}%;"></div>
          </div>
          <div class="quota-footer">
            <span>Consommation : ${formatCurrency(costVal)}</span>
            <span>Statut : ${escapeHtml(reset)}</span>
          </div>
        `;
        quotasContainer.appendChild(card);
      });

      // Carte de statut direct Google Antigravity
      if (agData && agData.session_count > 0) {
        const topModel = agData.top_models?.[0]?.model || "gemini-3.8-flash";
        const agCard = document.createElement("div");
        agCard.className = "quota-card";
        agCard.innerHTML = `
          <div class="quota-header">
            <span class="quota-title">ANTIGRAVITY (${topModel})</span>
            <span class="quota-percent">${agData.session_count} session${agData.session_count > 1 ? "s" : ""}</span>
          </div>
          <div class="progress-bar-bg">
            <div class="progress-bar-fill" style="width: 100%;"></div>
          </div>
          <div class="quota-footer">
            <span>Consommation : ${formatCurrency(agData.total_cost || 0)}</span>
            <span>Activité : ${formatNumber(agData.total_tokens || 0)} tokens</span>
          </div>
        `;
        quotasContainer.appendChild(agCard);
      }
    }
  }

  // Modèles les plus utilisés (fusion Codex, Cursor et Gemini Antigravity)
  const tbody = document.getElementById("models-tbody");
  if (tbody) {
    tbody.innerHTML = "";
    let mergedModels = [...(reportData.models || [])];
    if (agData?.top_models) {
      for (const agm of agData.top_models) {
        mergedModels.push({
          model: agm.model,
          totalTokens: agm.tokens,
          totalCost: agm.cost,
        });
      }
    }
    mergedModels.sort((a, b) => (b.totalTokens ?? b.tokens ?? 0) - (a.totalTokens ?? a.tokens ?? 0));

    if (mergedModels.length === 0) {
      tbody.innerHTML = `<tr><td colspan="3" style="text-align: center; color: var(--text-muted);">Aucune donnée de modèle disponible pour cette période.</td></tr>`;
    } else {
      mergedModels.forEach((m) => {
        const tr = document.createElement("tr");
        const modelName = m.model || m.name || "Inconnu";
        const modelTokens = m.totalTokens ?? m.tokens ?? 0;
        const modelCost = m.totalCost ?? m.cost ?? 0;
        tr.innerHTML = `
          <td><strong>${escapeHtml(modelName)}</strong></td>
          <td>${formatNumber(modelTokens)}</td>
          <td>${formatCurrency(modelCost)}</td>
        `;
        tbody.appendChild(tr);
      });
    }
  }
}

async function renderHarnessView(agentName) {
  const container = document.getElementById("harness-content");
  if (!container) return;

  container.innerHTML = `<div style="padding: 24px; text-align: center; color: var(--text-muted);">Chargement des données détaillées pour ${agentName.toUpperCase()}...</div>`;

  if (agentName === "cursor") {
    const subAgent = (reportData?.subscription?.agents || []).find((a) => a.agent?.toLowerCase() === "cursor");
    const cursorModels = (reportData?.models || []).filter((m) => m.model?.toLowerCase().includes("cursor"));
    const cursorTokens = cursorModels.reduce((acc, m) => acc + (m.totalTokens || m.tokens || 0), 0);
    const cursorCost = subAgent?.periodUsage ?? cursorModels.reduce((acc, m) => acc + (m.totalCost || m.cost || 0), 0);
    renderCursorHarness(container, subAgent, cursorModels, cursorCost, cursorTokens);
    return;
  }

  let hData = null;
  if (agentName === "codex" || agentName === "claude") {
    try {
      if (!harnessCache[agentName]) {
        harnessCache[agentName] = await invokeTauri("get_harness", { agent: agentName });
      }
      hData = harnessCache[agentName];
    } catch (e) {
      console.warn("Échec récupération harness pour", agentName, e);
    }
  }

  if (!hData || (!hData.plan && (!hData.topModels || hData.topModels.length === 0))) {
    container.innerHTML = `
      <div class="card settings-card">
        <h3>${agentName.toUpperCase()}</h3>
        <p style="color: var(--text-muted); margin-top: 10px;">
          Aucune session récente trouvée pour ${agentName} sur cette machine. Vérifiez que vous êtes bien connecté à l'assistant.
        </p>
      </div>
    `;
    return;
  }

  const econ = hData.economics || {};
  const win = hData.window || {};
  const remainingPct = win.usedPercent != null ? Math.max(0, 100 - win.usedPercent) : 100;
  const elapsedHrs = win.windowMinutes ? ((win.windowMinutes * (win.elapsedPercent || 0)) / 100 / 60).toFixed(1) : "0";

  // Consommation dynamique de l'agent sur la période active
  const agentPeriod = (periodCache[currentPeriod]?.agents || reportData?.agents || []).find((a) => a.agent === agentName);
  let agentCost = agentPeriod?.totalCost ?? 0;
  let agentTokens = agentPeriod?.totalTokens ?? 0;
  let agentModels = agentPeriod?.models || [];

  if (agentName === "codex" && agentTokens === 0) {
    const codexModels = (reportData?.models || []).filter((m) => !m.model?.toLowerCase().includes("cursor") && !m.model?.toLowerCase().includes("gemini"));
    agentTokens = codexModels.reduce((acc, m) => acc + (m.totalTokens || m.tokens || 0), 0);
    agentCost = codexModels.reduce((acc, m) => acc + (m.totalCost || m.cost || 0), 0);
    agentModels = codexModels;
  }

  const periodActivityHtml = `
    <div class="section-title-row" style="margin-top: 24px;">
      <div class="section-title">Consommation ${agentName.toUpperCase()} · <span style="color: var(--accent-flame);">${getPeriodLabel(currentPeriod)}</span></div>
    </div>
    <div class="metrics-grid">
      <div class="metric-card">
        <div class="metric-label">Tokens (${getPeriodLabel(currentPeriod)})</div>
        <div class="metric-value">${formatNumber(agentTokens)}</div>
        <div class="metric-sub">${agentTokens > 0 ? "Activité enregistrée" : "Aucune consommation sur cette timeline"}</div>
      </div>
      <div class="metric-card highlight">
        <div class="metric-label">Valeur consommée API</div>
        <div class="metric-value">${formatCurrency(agentCost)}</div>
        <div class="metric-sub">Coût équivalent au token API</div>
      </div>
      <div class="metric-card">
        <div class="metric-label">Modèles actifs (${getPeriodLabel(currentPeriod)})</div>
        <div class="metric-value">${agentModels.length}</div>
        <div class="metric-sub">${agentModels.map(m => m.model).slice(0, 2).join(", ") || "Aucun"}</div>
      </div>
    </div>
  `;

  let spendMixHtml = "";
  if (hData.spendMix && hData.spendMix.length > 0) {
    spendMixHtml = `
      <div class="section-title" style="margin-top: 24px;">Répartition globale des tokens (${agentName.toUpperCase()})</div>
      <div class="metrics-grid">
        ${hData.spendMix.map(m => `
          <div class="metric-card">
            <div class="metric-label">${m.label.toUpperCase()}</div>
            <div class="metric-value">${formatCurrency(m.costUSD)}</div>
            <div class="metric-sub">${formatNumber(m.tokens)} tokens (${m.tokenPercent.toFixed(1)}%)</div>
          </div>
        `).join("")}
      </div>
    `;
  }

  // Modèles sur la période active si disponibles, sinon modèles favoris globaux
  const displayModels = agentModels.length > 0 ? agentModels : (hData.topModels || []);
  const modelsTitle = agentModels.length > 0
    ? `Modèles utilisés par ${agentName.toUpperCase()} (${getPeriodLabel(currentPeriod)})`
    : `Modèles favoris de ${agentName.toUpperCase()}`;

  let topModelsHtml = "";
  if (displayModels.length > 0) {
    topModelsHtml = `
      <div class="section-title" style="margin-top: 24px;">${modelsTitle}</div>
      <div class="table-container">
        <table>
          <thead>
            <tr>
              <th>Modèle</th>
              <th>Tokens</th>
              <th>Coût équivalent</th>
            </tr>
          </thead>
          <tbody>
            ${displayModels.map(m => `
              <tr>
                <td><strong>${m.model || m.name}</strong></td>
                <td>${formatNumber(m.tokens ?? m.totalTokens ?? 0)}</td>
                <td>${formatCurrency(m.cost ?? m.totalCost ?? 0)}</td>
              </tr>
            `).join("")}
          </tbody>
        </table>
      </div>
    `;
  }

  let weeklyTrendHtml = "";
  if (hData.weeklyTrend && hData.weeklyTrend.length > 0) {
    weeklyTrendHtml = `
      <div class="section-title" style="margin-top: 24px;">Historique d'utilisation hebdomadaire (8 semaines)</div>
      <div class="table-container">
        <table>
          <thead>
            <tr>
              <th>Semaine débutant le</th>
              <th>Dépense équivalente</th>
            </tr>
          </thead>
          <tbody>
            ${hData.weeklyTrend.map(w => `
              <tr>
                <td>${w.weekStart}</td>
                <td><strong style="color: var(--accent-flame);">${formatCurrency(w.cost)}</strong></td>
              </tr>
            `).join("")}
          </tbody>
        </table>
      </div>
    `;
  }

  container.innerHTML = `
    <div class="card settings-card">
      <div style="display: flex; justify-content: space-between; align-items: center;">
        <h3 style="color: var(--accent-flame); font-size: 18px;">Compte ${agentName.toUpperCase()} (${hData.plan || "Plus"})</h3>
        <span class="badge active">${econ.pricePerMonth ? `$${econ.pricePerMonth}/mois` : "Actif"}</span>
      </div>

      <div class="metrics-grid" style="margin-top: 16px;">
        <div class="metric-card">
          <div class="metric-label">Quota hebdomadaire restant (Direct)</div>
          <div class="metric-value" style="color: var(--accent-flame);">${remainingPct.toFixed(0)}%</div>
          <div class="metric-sub">${win.usedPercent ?? 0}% utilisé (~${elapsedHrs}h écoulées)</div>
        </div>
        <div class="metric-card">
          <div class="metric-label">Dépense sur la fenêtre active</div>
          <div class="metric-value">${formatCurrency(win.apiEquivalentSpent || 0)}</div>
          <div class="metric-sub">${hData.resetCreditsAvailable != null ? `${hData.resetCreditsAvailable} crédits de reset` : "Dans le cycle"}</div>
        </div>
        <div class="metric-card highlight">
          <div class="metric-label">Multiplicateur de valeur (ROI)</div>
          <div class="metric-value">${econ.valueMultiple ? `${econ.valueMultiple.toFixed(1)}×` : "-"}</div>
          <div class="metric-sub">Subvention : ${formatCurrency(econ.subsidyPerMonth || 0)}/mois</div>
        </div>
      </div>
    </div>

    ${periodActivityHtml}
    ${spendMixHtml}
    ${topModelsHtml}
    ${weeklyTrendHtml}
  `;
}

function renderCursorHarness(container, subAgent, models, totalCost, totalTokens) {
  const planName = subAgent?.plan || "Usage détecté";
  container.innerHTML = `
    <div class="card settings-card">
      <div style="display: flex; justify-content: space-between; align-items: center;">
        <h3 style="color: var(--accent-flame); font-size: 18px;">Compte CURSOR</h3>
        <span class="badge active">${escapeHtml(planName)}</span>
      </div>

      <div class="metrics-grid" style="margin-top: 16px;">
        <div class="metric-card">
          <div class="metric-label">Consommation · ${getPeriodLabel(currentPeriod)}</div>
          <div class="metric-value" style="color: var(--accent-flame);">${formatCurrency(totalCost)}</div>
          <div class="metric-sub">Coût équivalent API calculé</div>
        </div>
        <div class="metric-card">
          <div class="metric-label">Tokens consommés</div>
          <div class="metric-value">${formatNumber(totalTokens)}</div>
          <div class="metric-sub">${models.length} modèle${models.length > 1 ? "s" : ""} actif${models.length > 1 ? "s" : ""}</div>
        </div>
      </div>
    </div>

    ${models.length > 0 ? `
      <div class="section-title" style="margin-top: 24px;">Modèles Cursor (${getPeriodLabel(currentPeriod)})</div>
      <div class="table-container">
        <table>
          <thead>
            <tr>
              <th>Modèle</th>
              <th>Tokens</th>
              <th>Coût équivalent</th>
            </tr>
          </thead>
          <tbody>
            ${models.map(m => `
              <tr>
                <td><strong>${escapeHtml(m.model || m.name)}</strong></td>
                <td>${formatNumber(m.totalTokens ?? m.tokens ?? 0)}</td>
                <td><strong style="color: var(--accent-flame);">${formatCurrency(m.totalCost ?? m.cost ?? 0)}</strong></td>
              </tr>
            `).join("")}
          </tbody>
        </table>
      </div>
    ` : `
      <div class="card settings-card" style="margin-top: 20px;">
        <p style="color: var(--text-muted);">Aucun modèle spécifique Cursor n'a été utilisé sur cette timeline.</p>
      </div>
    `}
  `;
}

function formatCurrency(num) {

  return "$" + Number(num).toFixed(2);
}

function formatNumber(num) {
  return Number(num).toLocaleString("fr-FR");
}

function renderDailyChart(daily) {
  const container = document.getElementById("chart-svg-container");
  if (!container) return;

  if (!daily || daily.length === 0) {
    container.innerHTML = `<div style="display:flex;height:100%;align-items:center;justify-content:center;color:var(--text-muted);font-size:13px;">Aucune donnée journalière disponible pour cette période.</div>`;
    return;
  }

  // Trier par date croissante
  const sorted = [...daily].sort((a, b) => a.date.localeCompare(b.date));

  const width = container.clientWidth || 980;
  const height = 200;
  const padLeft = 60;
  const padRight = 30;
  const padTop = 20;
  const padBottom = 30;

  const chartW = width - padLeft - padRight;
  const chartH = height - padTop - padBottom;

  const maxCost = Math.max(...sorted.map((d) => d.cost || 0), 1);

  // Calcul des coordonnées
  const points = sorted.map((d, i) => {
    const x = padLeft + (i / Math.max(sorted.length - 1, 1)) * chartW;
    const y = padTop + chartH - ((d.cost || 0) / maxCost) * chartH;
    return { x, y, data: d };
  });

  // Lignes de grille horizontales et labels d'ordonnées
  let gridLines = "";
  for (let step = 0; step <= 3; step++) {
    const ratio = step / 3;
    const yVal = padTop + chartH - ratio * chartH;
    const costLabel = "$" + (ratio * maxCost).toFixed(step === 0 ? 0 : 2);
    gridLines += `
      <line x1="${padLeft}" y1="${yVal}" x2="${padLeft + chartW}" y2="${yVal}" class="chart-grid-line" />
      <text x="${padLeft - 8}" y="${yVal + 3}" text-anchor="end" class="chart-axis-text">${costLabel}</text>
    `;
  }

  // Labels d'abscisses (dates)
  let xLabels = "";
  const stepX = Math.max(1, Math.ceil(sorted.length / 8));
  sorted.forEach((d, i) => {
    if (i % stepX === 0 || i === sorted.length - 1) {
      const pt = points[i];
      const parts = d.date.split("-");
      const shortDate = parts.length === 3 ? `${parts[2]}/${parts[1]}` : d.date;
      xLabels += `<text x="${pt.x}" y="${height - 8}" text-anchor="middle" class="chart-axis-text">${shortDate}</text>`;
    }
  });

  // Tracé de courbe lissée
  let pathD = "";
  if (points.length === 1) {
    pathD = `M ${points[0].x} ${points[0].y} L ${padLeft + chartW} ${points[0].y}`;
  } else {
    pathD = `M ${points[0].x} ${points[0].y}`;
    for (let i = 0; i < points.length - 1; i++) {
      const p0 = points[i];
      const p1 = points[i + 1];
      const mx = (p0.x + p1.x) / 2;
      pathD += ` C ${mx} ${p0.y}, ${mx} ${p1.y}, ${p1.x} ${p1.y}`;
    }
  }

  // Polygone d'aire fermée
  const lastPt = points[points.length - 1];
  const firstPt = points[0];
  const areaD = `${pathD} L ${lastPt.x} ${padTop + chartH} L ${firstPt.x} ${padTop + chartH} Z`;

  // Points interactifs
  let dots = "";
  points.forEach((pt, i) => {
    dots += `<circle cx="${pt.x}" cy="${pt.y}" r="4" class="chart-dot" data-idx="${i}" />`;
  });

  container.innerHTML = `
    <svg class="chart-svg" viewBox="0 0 ${width} ${height}">
      <defs>
        <linearGradient id="spendGradient" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0%" stop-color="#f97316" stop-opacity="0.35" />
          <stop offset="100%" stop-color="#f97316" stop-opacity="0.0" />
        </linearGradient>
      </defs>
      ${gridLines}
      ${xLabels}
      <path d="${areaD}" class="chart-area-spend" />
      <path d="${pathD}" class="chart-line-spend" />
      ${dots}
    </svg>
  `;

  // Écouteurs de survol
  const infoEl = document.getElementById("chart-hover-info");
  container.querySelectorAll(".chart-dot").forEach((dot) => {
    const idx = parseInt(dot.dataset.idx, 10);
    const d = sorted[idx];
    dot.addEventListener("mouseenter", () => {
      if (infoEl) {
        infoEl.textContent = `${d.date} : ${formatCurrency(d.cost || 0)} (${formatNumber(d.tokens || 0)} tokens)`;
      }
    });
    dot.addEventListener("mouseleave", () => {
      if (infoEl) {
        infoEl.textContent = "Survolez un point pour voir le détail";
      }
    });
  });
}

async function initAutostart() {
  const toggle = document.getElementById("autostart-toggle");
  if (!toggle) return;

  try {
    const isEnabled = await invokeTauri("get_autostart_status");
    toggle.checked = !!isEnabled;
  } catch (err) {
    console.warn("Erreur lecture autostart:", err);
  }

  toggle.addEventListener("change", async () => {
    try {
      const newState = await invokeTauri("set_autostart", { enabled: toggle.checked });
      toggle.checked = !!newState;
    } catch (err) {
      console.error("Erreur écriture autostart:", err);
      alert("Erreur lors de la modification du démarrage automatique: " + err);
    }
  });
}

function initDataFolder() {
  const btn = document.getElementById("open-folder-btn");
  if (btn) {
    btn.addEventListener("click", async () => {
      try {
        await invokeTauri("open_data_folder");
      } catch (err) {
        console.error("Erreur ouverture dossier:", err);
      }
    });
  }
}

function initCustomization() {
  // 1. Préférences des onglets (Cursor, Codex, Projets, Antigravity actifs par défaut)
  let visibleTabs = {
    summary: true,
    projects: true,
    antigravity: true,
    codex: true,
    claude: false,
    cursor: true,
  };

  try {
    const savedTabs = localStorage.getItem("agent_burn_visible_tabs");
    if (savedTabs) {
      visibleTabs = Object.assign(visibleTabs, JSON.parse(savedTabs));
      if (visibleTabs.cursor === undefined) {
        visibleTabs.cursor = true;
      }
    }
  } catch (_) {}

  // Appliquer sur les checkboxes et les boutons d'onglets
  document.querySelectorAll("#tabs-custom-container input[data-tab-target]").forEach((cb) => {
    const tabName = cb.dataset.tabTarget;
    if (visibleTabs[tabName] !== undefined) {
      cb.checked = !!visibleTabs[tabName];
    }
    applyTabVisibility(tabName, cb.checked);

    cb.addEventListener("change", () => {
      visibleTabs[tabName] = cb.checked;
      applyTabVisibility(tabName, cb.checked);
      try {
        localStorage.setItem("agent_burn_visible_tabs", JSON.stringify(visibleTabs));
      } catch (_) {}
    });
  });

  // 2. Préférences des timelines
  let visiblePeriods = {
    mtd: true,
    today: true,
    week: true,
    all: true,
  };

  try {
    const savedPeriods = localStorage.getItem("agent_burn_visible_periods");
    if (savedPeriods) {
      visiblePeriods = Object.assign(visiblePeriods, JSON.parse(savedPeriods));
    }
  } catch (_) {}

  document.querySelectorAll("#periods-custom-container input[data-period-target]").forEach((cb) => {
    const pName = cb.dataset.periodTarget;
    if (visiblePeriods[pName] !== undefined) {
      cb.checked = !!visiblePeriods[pName];
    }
    applyPeriodVisibility(pName, cb.checked);

    cb.addEventListener("change", () => {
      visiblePeriods[pName] = cb.checked;
      applyPeriodVisibility(pName, cb.checked);
      try {
        localStorage.setItem("agent_burn_visible_periods", JSON.stringify(visiblePeriods));
      } catch (_) {}
    });
  });
}

function applyTabVisibility(tabName, isVisible) {
  const btn = document.querySelector(`.tab-btn[data-tab="${tabName}"]`);
  if (btn) {
    btn.style.display = isVisible ? "" : "none";
  }
}

function applyPeriodVisibility(periodName, isVisible) {
  const btn = document.querySelector(`.period-btn[data-period="${periodName}"]`);
  if (btn) {
    btn.style.display = isVisible ? "" : "none";
  }
}


