import test from "node:test";
import assert from "node:assert/strict";

import {
  clearCachedEmails,
  getEmails,
  hydrateFolderCachesFromStorage,
  loadCachedEmails,
  saveCachedEmails,
} from "./cache.js";
import { EMAIL_CACHE_KEY } from "./constants.js";

function createStorage(initial = {}) {
  const store = new Map(Object.entries(initial));
  return {
    getItem(key) {
      return store.has(key) ? store.get(key) : null;
    },
    setItem(key, value) {
      store.set(key, String(value));
    },
    removeItem(key) {
      store.delete(key);
    },
  };
}

test("getEmails flattens folder caches", () => {
  const state = {
    cachedByFolder: {
      INBOX: [{ id: "1" }],
      Sent: [{ id: "2" }],
      Trash: [{ id: "3" }],
    },
  };

  assert.deepEqual(getEmails(state).map((x) => x.id), ["1", "2", "3"]);
});

test("loadCachedEmails handles missing and malformed payload", () => {
  const emptyStorage = createStorage();
  assert.deepEqual(loadCachedEmails(emptyStorage), []);

  const badStorage = createStorage({ [EMAIL_CACHE_KEY]: "not-json" });
  assert.deepEqual(loadCachedEmails(badStorage), []);
});

test("saveCachedEmails persists serialized payload", () => {
  const storage = createStorage();
  const state = {
    cachedByFolder: {
      INBOX: [{ id: "1" }],
      Sent: [{ id: "2" }],
      Trash: [],
    },
  };

  saveCachedEmails(state, storage);

  const raw = storage.getItem(EMAIL_CACHE_KEY);
  assert.ok(raw);
  const parsed = JSON.parse(raw);
  assert.equal(parsed.length, 2);
});

test("clearCachedEmails removes cache key", () => {
  const storage = createStorage({ [EMAIL_CACHE_KEY]: "[]" });
  clearCachedEmails(storage);
  assert.equal(storage.getItem(EMAIL_CACHE_KEY), null);
});

test("hydrateFolderCachesFromStorage categorizes folders consistently", () => {
  const storage = createStorage({
    [EMAIL_CACHE_KEY]: JSON.stringify([
      { id: "db-Sent-1", folder: "unknown", subject: "sent" },
      { id: "x-2", folder: "archive", subject: "archive" },
      { id: "x-3", folder: "inbox", subject: "inbox" },
    ]),
  });

  const state = {
    cachedByFolder: {
      INBOX: [{ id: "old" }],
      Sent: [{ id: "old" }],
      Trash: [{ id: "old" }],
    },
  };

  hydrateFolderCachesFromStorage(state, storage);

  assert.equal(state.cachedByFolder.Sent.length, 1);
  assert.equal(state.cachedByFolder.Sent[0].folder, "Sent");
  assert.equal(state.cachedByFolder.Trash.length, 1);
  assert.equal(state.cachedByFolder.Trash[0].folder, "Trash");
  assert.equal(state.cachedByFolder.INBOX.length, 1);
  assert.equal(state.cachedByFolder.INBOX[0].folder, "INBOX");
});
