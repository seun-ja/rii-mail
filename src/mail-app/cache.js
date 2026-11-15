import {
  EMAIL_CACHE_KEY,
  LEGACY_EMAIL_CACHE_KEYS,
  MAX_CACHED_EMAILS,
} from "./constants.js";

export function getEmails(state) {
  return [
    ...state.cachedByFolder.INBOX,
    ...(state.cachedByFolder.Sent ?? []),
    ...(state.cachedByFolder.Spam ?? []),
    ...(state.cachedByFolder.Trash ?? []),
  ];
}

export function loadCachedEmails(storage = window.localStorage) {
  try {
    const raw = storage.getItem(EMAIL_CACHE_KEY);
    if (raw) {
      const parsed = JSON.parse(raw);
      return Array.isArray(parsed) ? parsed : [];
    }

    for (const key of LEGACY_EMAIL_CACHE_KEYS) {
      const legacyRaw = storage.getItem(key);
      if (!legacyRaw) continue;

      const parsed = JSON.parse(legacyRaw);
      if (!Array.isArray(parsed)) continue;

      // Old attachment entries did not contain trustworthy IMAP part IDs.
      // Preserve the visible email list, but hide those stale controls until
      // the native background sync replaces the rows with current metadata.
      const migrated = parsed.map((email) => ({ ...email, attachments: [] }));
      storage.setItem(EMAIL_CACHE_KEY, JSON.stringify(migrated));
      LEGACY_EMAIL_CACHE_KEYS.forEach((legacyKey) =>
        storage.removeItem(legacyKey),
      );
      return migrated;
    }

    return [];
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
  LEGACY_EMAIL_CACHE_KEYS.forEach((key) => storage.removeItem(key));
}

export function hydrateFolderCachesFromStorage(
  state,
  storage = window.localStorage,
) {
  const cached = loadCachedEmails(storage);

  state.cachedByFolder.INBOX = [];
  state.cachedByFolder.Sent = [];
  state.cachedByFolder.Spam = [];
  state.cachedByFolder.Trash = [];

  cached.forEach((mail) => {
    const folder = String(mail?.folder || "").toLowerCase();

    if (
      folder === "sent" ||
      (typeof mail?.id === "string" && mail.id.startsWith("db-Sent-"))
    ) {
      state.cachedByFolder.Sent.push({ ...mail, folder: "Sent" });
    } else if (
      folder === "spam" ||
      (typeof mail?.id === "string" && mail.id.startsWith("db-Spam-"))
    ) {
      state.cachedByFolder.Spam.push({ ...mail, folder: "Spam" });
    } else if (folder === "trash" || folder === "archive") {
      state.cachedByFolder.Trash.push({ ...mail, folder: "Trash" });
    } else {
      state.cachedByFolder.INBOX.push({ ...mail, folder: "INBOX" });
    }
  });
}
