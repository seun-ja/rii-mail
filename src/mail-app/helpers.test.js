import test from "node:test";
import assert from "node:assert/strict";
import { runInNewContext } from "node:vm";

import {
  createForwardDraft,
  createReplyDraft,
  escapeHtml,
  formatAddressList,
  formatAddresses,
  getSafeExternalUrl,
  getActiveBackendFolderKey,
  getActiveMailboxLiteral,
  getFolderPaginationState,
  isMailListNearBottom,
  linkifyPlainText,
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

test("getSafeExternalUrl only allows web and mail links", () => {
  assert.equal(getSafeExternalUrl("https://example.com/path"), "https://example.com/path");
  assert.equal(getSafeExternalUrl("mailto:user@example.com"), "mailto:user@example.com");
  assert.equal(getSafeExternalUrl("javascript:alert(1)"), null);
  assert.equal(getSafeExternalUrl("/relative/path"), null);
});

test("linkifyPlainText creates escaped links for web addresses", () => {
  assert.equal(
    linkifyPlainText("Visit https://example.com?a=1&b=2 or www.example.org."),
    'Visit <a href="https://example.com/?a=1&amp;b=2" data-email-link>https://example.com?a=1&amp;b=2</a> or <a href="https://www.example.org/" data-email-link>www.example.org</a>.',
  );
  assert.equal(
    linkifyPlainText('<script>alert("no")</script>'),
    "&lt;script&gt;alert(&quot;no&quot;)&lt;/script&gt;",
  );
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
  assert.ok(result.indexOf("data-riimail-viewer-styles") < result.lastIndexOf("</head>"));
  assert.match(result, /<table width="900"><\/table>/);
});

test("prepareEmailHtml supports fragments and empty values", () => {
  assert.match(prepareEmailHtml("<p>Hello</p>"), /^<!doctype html>/);
  assert.match(prepareEmailHtml("<p>Hello</p>"), /<p>Hello<\/p>/);
  assert.equal(prepareEmailHtml("  "), "");
});

test("prepareEmailHtml puts a unique nonce policy and trusted bridge before email markup", () => {
  const source = '<script>untrusted()</script><html><head></head><body>Hello</body></html>';
  const result = prepareEmailHtml(source);
  const nonce = result.match(/<script nonce="([a-f0-9]+)">/)[1];

  assert.match(result, new RegExp(`script-src 'nonce-${nonce}'`));
  assert.match(result, /object-src 'none'; base-uri 'none'; form-action 'none'; frame-src 'none'/);
  assert.ok(result.indexOf("Content-Security-Policy") < result.indexOf("<script"));
  assert.ok(result.indexOf(`<script nonce="${nonce}">`) < result.indexOf("untrusted()"));
  assert.doesNotMatch(result, /unsafe-inline|unsafe-eval/);
  assert.notEqual(prepareEmailHtml(source).match(/<script nonce="([a-f0-9]+)">/)[1], nonce);
});

test("email bridge cancels navigation, forwards links, and scrolls local fragments", () => {
  const html = prepareEmailHtml("<p>Message</p>");
  const script = html.match(/<script nonce="[a-f0-9]+">([\s\S]*?)<\/script>/)[1];
  let onClick;
  const messages = [];
  const scrolledIds = [];
  runInNewContext(script, {
    document: {
      addEventListener(type, callback, capture) {
        assert.equal(type, "click");
        assert.equal(capture, true);
        onClick = callback;
      },
      getElementById(id) {
        return { scrollIntoView: () => scrolledIds.push(id) };
      },
    },
    window: { parent: { postMessage: (data, origin) => messages.push({ ...data, origin }) } },
  });

  const click = (url) => {
    let prevented = false;
    onClick({
      target: { closest: () => ({ getAttribute: () => url }) },
      preventDefault: () => { prevented = true; },
    });
    assert.equal(prevented, true);
  };
  click("https://example.com");
  click("mailto:user@example.com");
  click("#section%201");
  click("  #section2");
  click("#malformed%fragment");
  onClick({ target: { closest: () => null } });
  assert.deepEqual(messages, [
    { type: "riimail:open-email-link", url: "https://example.com", origin: "*" },
    { type: "riimail:open-email-link", url: "mailto:user@example.com", origin: "*" },
  ]);
  assert.deepEqual(scrolledIds, ["section 1", "section2"]);
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
