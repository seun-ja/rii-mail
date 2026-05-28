import test from "node:test";
import assert from "node:assert/strict";

import { createInitialState } from "./state.js";

test("createInitialState returns expected defaults", () => {
  const state = createInitialState();

  assert.equal(state.activeFolder, "INBOX");
  assert.deepEqual(Object.keys(state.cachedByFolder), ["INBOX", "Sent", "Trash"]);
  assert.equal(state.paginationByFolder.INBOX.nextOffset, 0);
  assert.equal(state.paginationByFolder.Sent.hasMoreEmails, true);
  assert.equal(state.paginationByFolder.Trash.lastFetchSignature, null);
  assert.equal(state.allowFetch, true);
  assert.equal(state.isInitialSyncComplete, false);
});
