const AVAILABLE_UPDATE_VERSION_KEY = "agent-burn-available-update-version";
const ANNOUNCED_UPDATE_VERSION_KEY = "agent-burn-announced-update-version";
let sessionAvailableUpdateVersion = null;
let sessionAnnouncedUpdateVersion = null;

export function createSafeStorage(getStorage) {
  const fallback = new Map();
  return {
    getItem(key) {
      try {
        const storage = getStorage();
        if (!storage) return fallback.get(key) ?? null;
        const value = storage.getItem(key);
        if (value == null) fallback.delete(key);
        else fallback.set(key, String(value));
        return value ?? null;
      } catch {
        return fallback.get(key) ?? null;
      }
    },
    setItem(key, value) {
      fallback.set(key, String(value));
      try {
        getStorage()?.setItem(key, value);
      } catch {}
    },
    removeItem(key) {
      fallback.delete(key);
      try {
        getStorage()?.removeItem(key);
      } catch {}
    },
  };
}

function normalizeVersion(version) {
  if (typeof version !== "string") return null;
  const normalized = version.trim();
  return /^v?\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$/.test(normalized)
    ? normalized
    : null;
}

export function getPersistedAvailableUpdate(storage) {
  try {
    if (!storage || typeof storage.getItem !== "function") return sessionAvailableUpdateVersion;
    const rawVersion = storage.getItem(AVAILABLE_UPDATE_VERSION_KEY);
    const version = normalizeVersion(rawVersion);
    sessionAvailableUpdateVersion = version;
    return version;
  } catch {}
  return sessionAvailableUpdateVersion;
}

export function rememberAvailableUpdate(storage, version) {
  const normalized = normalizeVersion(version);
  if (!normalized) return { version: null, shouldAnnounce: false };

  let previouslyAnnounced = sessionAnnouncedUpdateVersion;
  try {
    if (storage && typeof storage.getItem === "function") {
      previouslyAnnounced = storage.getItem(ANNOUNCED_UPDATE_VERSION_KEY) ?? null;
    }
  } catch {}
  const shouldAnnounce = previouslyAnnounced !== normalized;
  sessionAvailableUpdateVersion = normalized;
  if (shouldAnnounce) sessionAnnouncedUpdateVersion = normalized;
  try {
    storage?.setItem(AVAILABLE_UPDATE_VERSION_KEY, normalized);
    if (shouldAnnounce) storage?.setItem(ANNOUNCED_UPDATE_VERSION_KEY, normalized);
  } catch {}

  return { version: normalized, shouldAnnounce };
}

export function clearPersistedAvailableUpdate(storage) {
  sessionAvailableUpdateVersion = null;
  try {
    storage?.removeItem(AVAILABLE_UPDATE_VERSION_KEY);
  } catch {}
}
