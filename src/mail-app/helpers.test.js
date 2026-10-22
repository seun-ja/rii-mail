import test from "node:test";
import assert from "node:assert/strict";

import {
  escapeHtml,
  getActiveBackendFolderKey,
  getActiveMailboxLiteral,
  getFolderPaginationState,
  isMailListNearBottom,
  prepareEmailHtml,
  resetFolderPagination,
} from "./helpers.js";

test("escapeHtml safely encodes content inserted into the mail UI", () => {
  assert.equal(
    escapeHtml(`<script data-value="test">'unsafe' & text</script>`),
    "&lt;script data-value=&quot;test&quot;&gt;&#039;unsafe&#039; &amp; text&lt;/script&gt;",
  );
  assert.equal(escapeHtml(null), "");
});

test("prepareEmailHtml injects responsive viewer styles into a document head", () => {
  const source = "<html><head><title>Message</title></head><body><table width=\"900\"></table></body></html>";
  const result = prepareEmailHtml(source);

  assert.match(result, /data-riimail-viewer-styles/);
  assert.match(result, /max-width: 100% !important/);
  assert.ok(result.indexOf("data-riimail-viewer-styles") < result.indexOf("</head>"));
  assert.match(result, /<table width="900"><\/table>/);
});

test("prepareEmailHtml supports fragments and empty values", () => {
  assert.match(prepareEmailHtml("<p>Hello</p>"), /^\s*<meta name="viewport"/);
  assert.equal(prepareEmailHtml("  "), "");
});

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
