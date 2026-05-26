import { EMAIL_CACHE_KEY, MAX_CACHED_EMAILS } from "./constants.js";

export function getEmails(state) {
  return [
    ...state.cachedByFolder.INBOX,
    ...state.cachedByFolder.Sent,
    ...state.cachedByFolder.Trash,
  ];
}

export function loadCachedEmails(storage = window.localStorage) {
  try {
    const raw = storage.getItem(EMAIL_CACHE_KEY);
    if (!raw) {
      return [];
    }

    const parsed = JSON.parse(raw);
    if (!Array.isArray(parsed)) {
      return [];
    }

    return parsed;
  } catch (_error) {
    return [];
  }
}

export function saveCachedEmails(state, storage = window.localStorage) {
  try {
    const payload = getEmails(state).slice(0, MAX_CACHED_EMAILS);
    storage.setItem(EMAIL_CACHE_KEY, JSON.stringify(payload));
  } catch (_error) {
    // Ignore storage failures (quota/privacy mode).
  }
}

export function clearCachedEmails(storage = window.localStorage) {
  storage.removeItem(EMAIL_CACHE_KEY);
}

export function hydrateFolderCachesFromStorage(state, storage = window.localStorage) {
  const cached = loadCachedEmails(storage);

  state.cachedByFolder.INBOX = [];
  state.cachedByFolder.Sent = [];
  state.cachedByFolder.Trash = [];

  cached.forEach((mail) => {
    const folder = String(mail?.folder || "").toLowerCase();

    if (
      folder === "sent" ||
      (typeof mail?.id === "string" && mail.id.startsWith("db-Sent-"))
    ) {
      state.cachedByFolder.Sent.push({ ...mail, folder: "Sent" });
    } else if (folder === "trash" || folder === "archive") {
      state.cachedByFolder.Trash.push({ ...mail, folder: "Trash" });
    } else {
      state.cachedByFolder.INBOX.push({ ...mail, folder: "INBOX" });
    }
  });
}
