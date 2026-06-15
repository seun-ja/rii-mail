import { tauriSql } from "./tauri.js";

export const APP_SQLITE_DB_PATH_STORAGE_KEY = "riimail.sqliteDbPath";
export const DEFAULT_APP_SQLITE_DB_PATH = "sqlite:data/emails.db";

export function getAppSqliteDbPath(storage = window?.localStorage) {
  return (
    storage?.getItem(APP_SQLITE_DB_PATH_STORAGE_KEY) ||
    DEFAULT_APP_SQLITE_DB_PATH
  );
}

export function setAppSqliteDbPath(
  storage = window?.localStorage,
  dbPath = DEFAULT_APP_SQLITE_DB_PATH,
) {
  storage?.setItem(APP_SQLITE_DB_PATH_STORAGE_KEY, dbPath);
  return dbPath;
}

export function createAppSqliteClient({
  storage = window?.localStorage,
  sql = tauriSql,
} = {}) {
  let dbPromise = null;

  async function getDb() {
    if (!sql?.load) {
      return null;
    }

    if (!dbPromise) {
      const dbPath = getAppSqliteDbPath(storage);
      dbPromise = sql.load(dbPath).catch((error) => {
        dbPromise = null;
        throw error;
      });
    }

    return dbPromise;
  }

  return {
    getDb,
  };
}
