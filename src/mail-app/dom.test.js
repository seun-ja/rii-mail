import test from "node:test";
import assert from "node:assert/strict";

import { getMailAppDom } from "./dom.js";

test("getMailAppDom queries all expected selectors", () => {
  const calls = [];

  const fakeDocument = {
    querySelectorAll(selector) {
      calls.push(["all", selector]);
      return [selector];
    },
    querySelector(selector) {
      calls.push(["one", selector]);
      return { selector };
    },
  };

  const dom = getMailAppDom(fakeDocument);

  assert.deepEqual(dom.folderButtons, [".folder-btn"]);
  assert.equal(dom.mailListEl.selector, "#mail-list");
  assert.equal(dom.mailReaderEl.selector, "#mail-reader");
  assert.equal(dom.syncProgressBarEl.selector, "#sync-progress-bar");

  assert.ok(calls.some(([, selector]) => selector === "#mail-reader"));
  assert.ok(calls.some(([, selector]) => selector === "#sync-overlay"));
});
