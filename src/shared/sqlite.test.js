import test from "node:test";
import assert from "node:assert/strict";

import {
  APP_SQLITE_DB_PATH_STORAGE_KEY,
  DEFAULT_APP_SQLITE_DB_PATH,
  createAppSqliteClient,
  getAppSqliteDbPath,
  setAppSqliteDbPath,
} from "./sqlite.js";

function createStorage(initial = {}) {
  const store = new Map(Object.entries(initial));
  return {
    getItem(key) {
      return store.has(key) ? store.get(key) : null;
    },
    setItem(key, value) {
      store.set(key, String(value));
    },
  };
}

test("getAppSqliteDbPath returns stored path when present", () => {
  const storage = createStorage({
    [APP_SQLITE_DB_PATH_STORAGE_KEY]: "sqlite:data/custom.db",
  });

  assert.equal(getAppSqliteDbPath(storage), "sqlite:data/custom.db");
});

test("getAppSqliteDbPath falls back to default path", () => {
  assert.equal(getAppSqliteDbPath(createStorage()), DEFAULT_APP_SQLITE_DB_PATH);
});

test("setAppSqliteDbPath persists the default db path", () => {
  const storage = createStorage();

  const dbPath = setAppSqliteDbPath(storage);

  assert.equal(dbPath, DEFAULT_APP_SQLITE_DB_PATH);
  assert.equal(
    storage.getItem(APP_SQLITE_DB_PATH_STORAGE_KEY),
    DEFAULT_APP_SQLITE_DB_PATH,
  );
});

test("createAppSqliteClient loads and caches the app db connection", async () => {
  const loadCalls = [];
  const db = { select: async () => [] };
  const client = createAppSqliteClient({
    storage: createStorage({
      [APP_SQLITE_DB_PATH_STORAGE_KEY]: "sqlite:data/custom.db",
    }),
    sql: {
      load: async (path) => {
        loadCalls.push(path);
        return db;
      },
    },
  });

  const first = await client.getDb();
  const second = await client.getDb();

  assert.equal(first, db);
  assert.equal(second, db);
  assert.deepEqual(loadCalls, ["sqlite:data/custom.db"]);
});
