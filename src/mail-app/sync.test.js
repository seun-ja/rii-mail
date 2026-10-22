import test from "node:test";
import assert from "node:assert/strict";

import { createInitialState } from "./state.js";
import { createSyncController } from "./sync.js";

function createStorage() {
  const values = new Map([["riimail.provider", "custom"]]);

  return {
    getItem(key) {
      return values.get(key) ?? null;
    },
    setItem(key, value) {
      values.set(key, String(value));
    },
    removeItem(key) {
      values.delete(key);
    },
  };
}

test("refresh reconciles the loaded page with newest emails first", async () => {
  const state = createInitialState();
  state.allowFetch = true;
  state.isAppReady = true;
  state.cachedByFolder.INBOX = [
    { id: "db-INBOX-0", subject: "Previously newest", folder: "INBOX" },
    { id: "db-INBOX-1", subject: "Older", folder: "INBOX" },
  ];
  state.paginationByFolder.INBOX.nextOffset = 2;

  const refreshedPage = [
    { id: "db-INBOX-uid-103", subject: "New", folder: "INBOX" },
    { id: "db-INBOX-uid-102", subject: "Previously newest", folder: "INBOX" },
    { id: "db-INBOX-uid-101", subject: "Older", folder: "INBOX" },
  ];
  const calls = [];
  const invoke = async (command, args) => {
    calls.push({ command, args });

    if (command === "refresh_emails_handler") {
      return {
        emails: [refreshedPage[0]],
        newEmailsCount: 1,
        totalEmails: 3,
      };
    }

    if (command === "fetch_emails") {
      return { emails: refreshedPage };
    }

    throw new Error(`Unexpected command: ${command}`);
  };
  const renderer = { renderList() {} };
  const ui = {};
  const storage = createStorage();
  const sync = createSyncController(state, renderer, ui, invoke, storage);

  const result = await sync.refreshActiveMailbox();

  assert.equal(result.newCount, 1);
  assert.equal(result.totalEmails, 3);
  assert.deepEqual(
    state.cachedByFolder.INBOX.map((email) => email.id),
    ["db-INBOX-uid-103", "db-INBOX-uid-102", "db-INBOX-uid-101"],
  );
  assert.equal(state.paginationByFolder.INBOX.nextOffset, 3);
  assert.equal(state.paginationByFolder.INBOX.hasMoreEmails, false);
  assert.equal(calls[0].command, "refresh_emails_handler");
  assert.equal(calls[1].command, "fetch_emails");
  assert.equal(calls[1].args.minRange, 0);
  assert.ok(calls[1].args.maxRange >= refreshedPage.length);
});
