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
  assert.equal(dom.searchInputEl.selector, "#global-search");
  assert.equal(dom.syncProgressBarEl.selector, "#sync-progress-bar");
  assert.equal(dom.openAiAssistantBtnEl.selector, "#open-ai-assistant-btn");
  assert.equal(dom.aiAssistantPanelEl.selector, "#ai-assistant-panel");
  assert.deepEqual(dom.settingsNavButtons, ["[data-settings-section]"]);
  assert.deepEqual(dom.settingsPanels, ["[data-settings-panel]"]);
  assert.equal(dom.signatureInputEl.selector, "#signature-input");
  assert.equal(dom.signatureToolbarEl.selector, "#signature-toolbar");
  assert.equal(dom.signatureFontSelectEl.selector, "#signature-font");

  assert.ok(calls.some(([, selector]) => selector === "#mail-reader"));
  assert.ok(calls.some(([, selector]) => selector === "#sync-overlay"));
});
