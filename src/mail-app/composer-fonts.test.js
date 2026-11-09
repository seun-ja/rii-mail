import test from "node:test";
import assert from "node:assert/strict";

import {
  DEFAULT_COMPOSER_FONT,
  createComposerHtmlBody,
  getComposerFont,
  initializeComposerEditor,
  plainTextToComposerHtml,
  setComposerText,
} from "./composer-fonts.js";

test("uses Aptos as the default and rejects unknown font values", () => {
  assert.equal(getComposerFont().value, DEFAULT_COMPOSER_FONT);
  assert.equal(getComposerFont("not-a-font").value, DEFAULT_COMPOSER_FONT);
});

test("escapes plain text before inserting it into the rich-text editor", () => {
  assert.equal(
    plainTextToComposerHtml("Hello <team> & all\nSecond line"),
    "Hello &lt;team&gt; &amp; all<br>Second line",
  );

  const editor = { innerHTML: "" };
  setComposerText(editor, "<strong>not markup</strong>");
  assert.equal(editor.innerHTML, "&lt;strong&gt;not markup&lt;/strong&gt;");
});

test("wraps rich editor HTML in the default Aptos email style", () => {
  assert.equal(
    createComposerHtmlBody("Hello <strong>team</strong>"),
    '<div style="font-family: Aptos, Arial, sans-serif; font-size: 11pt; line-height: 1.5;">Hello <strong>team</strong></div>',
  );
});

test("applies a chosen font to the saved editor selection", () => {
  const editorListeners = {};
  const toolbarListeners = {};
  const fontListeners = {};
  const anchorNode = {};
  const range = { cloneRange: () => range };
  const selection = {
    rangeCount: 1,
    anchorNode,
    getRangeAt: () => range,
    removeAllRanges() {},
    addRange() {},
  };
  const commands = [];
  const editor = {
    style: {},
    contains: (node) => node === anchorNode,
    addEventListener: (type, listener) => {
      editorListeners[type] = listener;
    },
    focus() {},
  };
  const toolbar = {
    addEventListener: (type, listener) => {
      toolbarListeners[type] = listener;
    },
    querySelectorAll: () => [],
  };
  const fontSelect = {
    value: "aptos",
    addEventListener: (type, listener) => {
      fontListeners[type] = listener;
    },
  };
  const documentRef = {
    addEventListener() {},
    getSelection: () => selection,
    execCommand: (...args) => commands.push(args),
  };

  initializeComposerEditor({ editor, toolbar, fontSelect, documentRef });
  editorListeners.mouseup();
  fontSelect.value = "georgia";
  fontListeners.change();

  assert.equal(editor.style.fontFamily, '"Aptos", Arial, sans-serif');
  assert.deepEqual(commands.at(-1), ["fontName", false, "Georgia"]);
});
