import test from "node:test";
import assert from "node:assert/strict";

import {
  createForwardDraft,
  createReplyDraft,
  escapeHtml,
  formatAddressList,
  formatAddresses,
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

test("formatAddressList formats named and bare email addresses", () => {
  assert.equal(
    formatAddressList([
      ["Bob", "bob@example.com"],
      ["", "carol@example.com"],
      null,
    ]),
    "Bob <bob@example.com>, carol@example.com",
  );
  assert.deepEqual(formatAddresses(null), []);
  assert.deepEqual(formatAddresses([["Bob", "bob@example.com"]]), [
    "Bob <bob@example.com>",
  ]);
  assert.equal(formatAddressList(null), "");
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

test("createReplyDraft addresses the sender, preserves reply subjects, and quotes plain text", () => {
  const draft = createReplyDraft({
    senderName: "Alex Example",
    emailFrom: "alex@example.com",
    subject: "Re: Project update",
    time: "Oct 27 10:30",
    textBody: "First line\nSecond line",
  });

  assert.deepEqual(draft, {
    to: "alex@example.com",
    subject: "Re: Project update",
    body: "\n\nOn Oct 27 10:30, Alex Example <alex@example.com> wrote:\n> First line\n> Second line",
  });
});

test("createForwardDraft leaves recipients empty and formats HTML-only emails as text", () => {
  const draft = createForwardDraft({
    senderName: "Alex Example",
    emailFrom: "alex@example.com",
    subject: "Project update",
    time: "Oct 27 10:30",
    htmlBody: "<p>Hello<br>team &amp; friends</p>",
    cc: [
      ["Bob", "bob@example.com"],
      ["", "carol@example.com"],
    ],
  });

  assert.equal(draft.to, "");
  assert.equal(draft.subject, "Fwd: Project update");
  assert.equal(
    draft.body,
    "\n\n---------- Forwarded message ---------\nFrom: Alex Example <alex@example.com>\nDate: Oct 27 10:30\nCc: Bob <bob@example.com>, carol@example.com\nSubject: Project update\n\nHello\nteam & friends",
  );
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
