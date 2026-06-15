import test from "node:test";
import assert from "node:assert/strict";

import { createMailboxCountClient } from "./mailbox-counts.js";

function createStorage(initial = {}) {
  const store = new Map(Object.entries(initial));
  return {
    getItem(key) {
      return store.has(key) ? store.get(key) : null;
    },
  };
}

test("getMailboxCountDb loads the configured sqlite path once", async () => {
  const loadCalls = [];
  const db = { select: async () => [] };
  const client = createMailboxCountClient({
    storage: createStorage({ "riimail.sqliteDbPath": "sqlite:data/custom.db" }),
    sql: {
      load: async (path) => {
        loadCalls.push(path);
        return db;
      },
    },
  });

  const first = await client.getMailboxCountDb();
  const second = await client.getMailboxCountDb();

  assert.equal(first, db);
  assert.equal(second, db);
  assert.deepEqual(loadCalls, ["sqlite:data/custom.db"]);
});

test("fetchMailboxEmailCount queries the expected mailbox column", async () => {
  const selectCalls = [];
  const client = createMailboxCountClient({
    storage: createStorage(),
    sql: {
      load: async () => ({
        select: async (query, params) => {
          selectCalls.push({ query, params });
          return [{ count: 42 }];
        },
      }),
    },
  });

  const count = await client.fetchMailboxEmailCount("INBOX", "gmail");

  assert.equal(count, 42);
  assert.deepEqual(selectCalls, [
    {
      query: "SELECT INBOX AS count FROM total_emails WHERE provider = $1",
      params: ["gmail"],
    },
  ]);
});

test("fetchMailboxEmailCount returns null for unsupported mailbox names", async () => {
  const client = createMailboxCountClient({
    storage: createStorage(),
    sql: {
      load: async () => ({
        select: async () => {
          throw new Error("select should not be called");
        },
      }),
    },
  });

  const count = await client.fetchMailboxEmailCount("Trash", "gmail");

  assert.equal(count, null);
});
