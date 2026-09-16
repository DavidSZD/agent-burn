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

export function subscriptionPresentation(subscription) {
  const rawPlan = String(subscription?.plan || "").trim();
  const unknownPlan = !rawPlan || /^(unknown|plan not detected)$/i.test(rawPlan);
  const price = Number(subscription?.pricePerMonth);
  return {
    plan: unknownPlan ? "Free" : rawPlan,
    monthlyPrice: Number.isFinite(price) && price > 0 ? price : null,
  };
}

export function shouldShowTimelineLoading(periodCache, period) {
  return !Object.prototype.hasOwnProperty.call(periodCache, period);
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
