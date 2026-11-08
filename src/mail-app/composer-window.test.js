import test from "node:test";
import assert from "node:assert/strict";

import { readDetachedComposerDraft } from "./composer-window.js";

function createStorage(entries = {}) {
  const values = new Map(Object.entries(entries));
  return {
    getItem(key) {
      return values.has(key) ? values.get(key) : null;
    },
    removeItem(key) {
      values.delete(key);
    },
    has(key) {
      return values.has(key);
    },
  };
}

test("reads and immediately removes a staged detached-composer draft", () => {
  const key = "riimail.detachedComposer.123";
  const draft = { title: "Reply", to: "person@example.com", isReply: true };
  const storage = createStorage({ [key]: JSON.stringify(draft) });

  assert.deepEqual(
    readDetachedComposerDraft(storage, `#${encodeURIComponent(key)}`),
    draft,
  );
  assert.equal(storage.has(key), false);
});

test("rejects unrelated, missing, and malformed staged drafts", () => {
  const malformedKey = "riimail.detachedComposer.bad";
  const storage = createStorage({ [malformedKey]: "not json" });

  assert.equal(readDetachedComposerDraft(storage, "#unrelated"), null);
  assert.equal(readDetachedComposerDraft(storage, "#%E0%A4%A"), null);
  assert.equal(readDetachedComposerDraft(storage, `#${malformedKey}`), null);
  assert.equal(storage.has(malformedKey), false);
});
