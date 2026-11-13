import test from "node:test";
import assert from "node:assert/strict";

import {
  createDetachedComposerHtml,
  readDetachedComposerDraft,
} from "./composer-window.js";

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

test("preserves the exact visible HTML when moving a draft to a detached composer", () => {
  const editedHtml = "Message without the deleted signature";

  assert.equal(
    createDetachedComposerHtml({
      title: "New Message",
      bodyHtml: editedHtml,
      signature: "This must not be restored",
    }),
    editedHtml,
  );
});

test("adds a configured signature when a detached draft has only plain text", () => {
  assert.equal(
    createDetachedComposerHtml({
      title: "Reply",
      body: "On Monday, Person wrote:",
      signature: "Regards,\nAminu",
    }),
    '<div><br></div><div data-riimail-signature="true">Regards,<br>Aminu</div><br><br>On Monday, Person wrote:',
  );
});

test("rejects unrelated, missing, and malformed staged drafts", () => {
  const malformedKey = "riimail.detachedComposer.bad";
  const storage = createStorage({ [malformedKey]: "not json" });

  assert.equal(readDetachedComposerDraft(storage, "#unrelated"), null);
  assert.equal(readDetachedComposerDraft(storage, "#%E0%A4%A"), null);
  assert.equal(readDetachedComposerDraft(storage, `#${malformedKey}`), null);
  assert.equal(storage.has(malformedKey), false);
});
