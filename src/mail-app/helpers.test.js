import test from "node:test";
import assert from "node:assert/strict";

import {
  getActiveBackendFolderKey,
  getActiveMailboxLiteral,
  getFolderPaginationState,
  isMailListNearBottom,
  resetFolderPagination,
} from "./helpers.js";

test("folder helpers map frontend folder to backend mailbox", () => {
  assert.equal(getActiveBackendFolderKey({ activeFolder: "INBOX" }), "INBOX");
  assert.equal(getActiveBackendFolderKey({ activeFolder: "Sent" }), "Sent");
  assert.equal(getActiveBackendFolderKey({ activeFolder: "Trash" }), "Trash");
  assert.equal(getActiveMailboxLiteral({ activeFolder: "INBOX" }), "INBOX");
});

test("getFolderPaginationState returns selected folder pagination", () => {
  const state = {
    activeFolder: "INBOX",
    paginationByFolder: {
      INBOX: { nextOffset: 10 },
      Sent: { nextOffset: 20 },
    },
  };

  assert.equal(getFolderPaginationState(state).nextOffset, 10);
  assert.equal(getFolderPaginationState(state, "Sent").nextOffset, 20);
});

test("resetFolderPagination resets known folder values", () => {
  const state = {
    paginationByFolder: {
      INBOX: {
        nextOffset: 90,
        hasMoreEmails: false,
        lastFetchSignature: "sig",
        lastFetchAt: 123,
      },
    },
  };

  resetFolderPagination(state, "INBOX");

  assert.deepEqual(state.paginationByFolder.INBOX, {
    nextOffset: 0,
    hasMoreEmails: true,
    lastFetchSignature: null,
    lastFetchAt: 0,
  });
});

test("resetFolderPagination is a no-op for unknown folder", () => {
  const state = { paginationByFolder: {} };
  resetFolderPagination(state, "Unknown");
  assert.deepEqual(state, { paginationByFolder: {} });
});

test("isMailListNearBottom detects when the scroll threshold is reached", () => {
  assert.equal(
    isMailListNearBottom({
      scrollTop: 560,
      clientHeight: 400,
      scrollHeight: 1000,
    }),
    true,
  );

  assert.equal(
    isMailListNearBottom({
      scrollTop: 500,
      clientHeight: 400,
      scrollHeight: 1000,
    }),
    false,
  );

  assert.equal(isMailListNearBottom(null), false);
});
