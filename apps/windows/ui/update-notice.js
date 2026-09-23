const AVAILABLE_UPDATE_VERSION_KEY = "agent-burn-available-update-version";
const ANNOUNCED_UPDATE_VERSION_KEY = "agent-burn-announced-update-version";

function normalizeVersion(version) {
  if (typeof version !== "string") return null;
  const normalized = version.trim();
  return /^v?\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$/.test(normalized)
    ? normalized
    : null;
}

export function getPersistedAvailableUpdate(storage) {
  return normalizeVersion(storage?.getItem(AVAILABLE_UPDATE_VERSION_KEY));
}

export function rememberAvailableUpdate(storage, version) {
  const normalized = normalizeVersion(version);
  if (!storage || !normalized) return { version: null, shouldAnnounce: false };

  const previouslyAnnounced = storage.getItem(ANNOUNCED_UPDATE_VERSION_KEY);
  storage.setItem(AVAILABLE_UPDATE_VERSION_KEY, normalized);
  const shouldAnnounce = previouslyAnnounced !== normalized;
  if (shouldAnnounce) storage.setItem(ANNOUNCED_UPDATE_VERSION_KEY, normalized);

  return { version: normalized, shouldAnnounce };
}

export function clearPersistedAvailableUpdate(storage) {
  storage?.removeItem(AVAILABLE_UPDATE_VERSION_KEY);
}
