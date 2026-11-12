import test from "node:test";
import assert from "node:assert/strict";

import {
  containsSignature,
  createGeneratedDraftHtml,
  createSignatureHtml,
  createSignedDraftHtml,
  getAuthoredComposerText,
  getComposerSignatureHtml,
  normalizeSignature,
  normalizeSignatureUrl,
  sanitizeSignatureHtml,
} from "./signature.js";

test("normalizes empty signatures and preserves legacy plain-text line breaks", () => {
  assert.equal(normalizeSignature(null), "");
  assert.equal(normalizeSignature("  \n  "), "");
  assert.equal(normalizeSignature("Kind regards,\nAminu"), "Kind regards,<br>Aminu");
});

test("allows safe link and remote-image URLs only", () => {
  assert.equal(normalizeSignatureUrl("https://example.com/about"), "https://example.com/about");
  assert.equal(normalizeSignatureUrl("mailto:hello@example.com"), "mailto:hello@example.com");
  assert.equal(normalizeSignatureUrl("javascript:alert(1)"), null);
  assert.equal(
    normalizeSignatureUrl("https://example.com/logo.png", { image: true }),
    "https://example.com/logo.png",
  );
  assert.equal(normalizeSignatureUrl("mailto:hello@example.com", { image: true }), null);
});

test("falls back to escaped text when DOM sanitization is unavailable", () => {
  const result = sanitizeSignatureHtml('<img src="x" onerror="alert(1)">');

  assert.equal(result.includes("<img"), false);
  assert.equal(result.includes("&lt;img"), true);
});

test("wraps already-sanitized rich signature HTML", () => {
  assert.equal(
    createSignatureHtml('<strong>Thanks</strong><br><a href="https://example.com">Website</a>'),
    '<div data-riimail-signature="true"><strong>Thanks</strong><br><a href="https://example.com">Website</a></div>',
  );
});

test("adds a rich signature below a new message and only once", () => {
  const signature = "<strong>Regards,</strong><br>Aminu";
  const html = createSignedDraftHtml({ body: "Hello team", signature });

  assert.equal(
    html,
    'Hello team<br><br><div data-riimail-signature="true"><strong>Regards,</strong><br>Aminu</div>',
  );
  assert.equal(containsSignature(html), true);
  assert.equal(createSignedDraftHtml({ bodyHtml: html, signature }), html);
});

test("places the rich signature before reply or forward content", () => {
  assert.equal(
    createSignedDraftHtml({
      body: "On Monday, Person wrote:\n> Original message",
      signature: "Regards,<br><a href=\"https://example.com\">Aminu</a>",
      signatureBeforeBody: true,
    }),
    '<div><br></div><div data-riimail-signature="true">Regards,<br><a href="https://example.com">Aminu</a></div><br><br>On Monday, Person wrote:<br>&gt; Original message',
  );
});

test("reads authored text without counting the inserted signature", () => {
  const clone = {
    innerText: "Message body\nSaved signature",
    querySelector() {
      return {
        remove() {
          clone.innerText = "Message body";
        },
      };
    },
  };
  const editor = { cloneNode: () => clone };

  assert.equal(getAuthoredComposerText(editor), "Message body");
});

test("distinguishes an edited signature from a removed signature", () => {
  assert.equal(
    getComposerSignatureHtml({
      querySelector: () => ({ innerHTML: "Edited signature" }),
    }),
    "Edited signature",
  );
  assert.equal(getComposerSignatureHtml({ querySelector: () => null }), null);
});

test("preserves the existing reply spacing when no signature is configured", () => {
  assert.equal(
    createSignedDraftHtml({ body: "\n\nOn Monday, Person wrote:" }),
    "<br><br>On Monday, Person wrote:",
  );
});

test("keeps the rich signature between an AI response and quoted content", () => {
  assert.equal(
    createGeneratedDraftHtml({
      body: "Generated response",
      signature: "<strong>Regards,</strong><br>Aminu",
      trailingContent: "On Monday, Person wrote:\n> Original",
    }),
    'Generated response<br><br><div data-riimail-signature="true"><strong>Regards,</strong><br>Aminu</div><br><br>On Monday, Person wrote:<br>&gt; Original',
  );
});
