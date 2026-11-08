import test from "node:test";
import assert from "node:assert/strict";

import { createRenderer } from "./render.js";

function createState(email) {
  return {
    selectedId: email.id,
    cachedByFolder: {
      INBOX: [email],
      Sent: [],
      Trash: [],
    },
  };
}

function createReaderElement() {
  return {
    className: "",
    innerHTML: "",
    querySelector() {
      return null;
    },
  };
}

test("renderReader shows escaped Cc recipients without mutating read state", () => {
  const email = {
    id: "message-1",
    folder: "INBOX",
    senderName: "Alice",
    emailFrom: "alice@example.com",
    cc: [
      ["Bob & Co", "bob@example.com"],
      ["", "carol@example.com"],
    ],
    subject: "Project update",
    body: "Hello",
    textBody: "Hello",
    htmlBody: "",
    time: "Oct 27 10:30",
    read: false,
  };
  const mailReaderEl = createReaderElement();
  const renderer = createRenderer(createState(email), { mailReaderEl });

  renderer.renderReader();

  assert.match(mailReaderEl.innerHTML, /data-reader-action="reply"/);
  assert.match(mailReaderEl.innerHTML, /data-reader-action="forward"/);
  assert.match(mailReaderEl.innerHTML, /data-reader-action="trash"/);
  assert.match(mailReaderEl.innerHTML, /class="reader-meta-item reader-meta-cc"/);
  assert.match(
    mailReaderEl.innerHTML,
    /Bob &amp; Co &lt;bob@example\.com&gt;, carol@example\.com/,
  );
  assert.equal(email.read, false);
});

test("renderReader collapses long Cc recipient lists", () => {
  const email = {
    id: "message-many-cc",
    folder: "INBOX",
    senderName: "Alice",
    emailFrom: "alice@example.com",
    cc: [
      ["Bob", "bob@example.com"],
      ["Carol", "carol@example.com"],
      ["Dan", "dan@example.com"],
      ["Eve", "eve@example.com"],
    ],
    subject: "Project update",
    body: "Hello",
    textBody: "Hello",
    htmlBody: "",
    time: "Oct 27 10:30",
  };
  const mailReaderEl = createReaderElement();
  const renderer = createRenderer(createState(email), { mailReaderEl });

  renderer.renderReader();

  assert.match(mailReaderEl.innerHTML, /<details[^>]+reader-meta-cc-collapsible/);
  assert.match(mailReaderEl.innerHTML, /\+2 more/);
  assert.match(
    mailReaderEl.innerHTML,
    /reader-meta-cc-expanded[^>]*>Bob &lt;bob@example\.com&gt;, Carol &lt;carol@example\.com&gt;, Dan &lt;dan@example\.com&gt;, Eve &lt;eve@example\.com&gt;/,
  );
});

test("renderReader omits the Cc row when there are no recipients", () => {
  const email = {
    id: "message-2",
    folder: "INBOX",
    senderName: "Alice",
    emailFrom: "alice@example.com",
    cc: null,
    subject: "Project update",
    body: "Hello",
    textBody: "Hello",
    htmlBody: "",
    time: "Oct 27 10:30",
  };
  const mailReaderEl = createReaderElement();
  const renderer = createRenderer(createState(email), { mailReaderEl });

  renderer.renderReader();

  assert.doesNotMatch(mailReaderEl.innerHTML, /reader-meta-cc/);
});

test("renderReader makes plain-text web links clickable", () => {
  const email = {
    id: "message-links",
    subject: "Links",
    body: "Visit https://example.com",
    textBody: "Visit https://example.com",
    htmlBody: "",
  };
  const mailReaderEl = createReaderElement();
  const renderer = createRenderer(createState(email), { mailReaderEl });

  renderer.renderReader();

  assert.match(
    mailReaderEl.innerHTML,
    /<a href="https:\/\/example\.com\/" data-email-link>https:\/\/example\.com<\/a>/,
  );
});

test("renderReader only opens safe messages from its isolated HTML frame", () => {
  const listeners = new Set();
  const hostWindow = {
    addEventListener(eventName, callback) {
      assert.equal(eventName, "message");
      listeners.add(callback);
    },
    removeEventListener(eventName, callback) {
      assert.equal(eventName, "message");
      listeners.delete(callback);
    },
  };
  const frame = { contentWindow: {} };
  const email = {
    id: "message-html-links",
    subject: "Links",
    body: "",
    textBody: "",
    htmlBody: '<a href="https://example.com">Visit</a>',
  };
  const mailReaderEl = {
    ...createReaderElement(),
    ownerDocument: { defaultView: hostWindow },
    querySelector(selector) {
      return selector === ".reader-html-frame" ? frame : null;
    },
  };
  const openedUrls = [];
  const renderer = createRenderer(createState(email), { mailReaderEl }, (url) => {
    openedUrls.push(url);
  });

  renderer.renderReader();
  assert.match(mailReaderEl.innerHTML, /sandbox="allow-scripts"/);
  assert.doesNotMatch(mailReaderEl.innerHTML, /allow-same-origin/);
  assert.match(frame.srcdoc, /Content-Security-Policy/);

  const [onMessage] = listeners;
  const message = (url, source = frame.contentWindow, type = "riimail:open-email-link") => ({
    source,
    data: { type, url },
  });
  onMessage(message("https://example.com"));
  onMessage(message("mailto:user@example.com"));
  onMessage(message("javascript:alert(1)"));
  onMessage(message("file:///etc/passwd"));
  onMessage(message("/relative/path"));
  onMessage(message("https://untrusted.example", {}));
  onMessage(message("https://untrusted.example", frame.contentWindow, "other"));
  onMessage(message({ href: "https://untrusted.example" }));
  onMessage({ source: frame.contentWindow, data: null });
  assert.deepEqual(openedUrls, ["https://example.com/", "mailto:user@example.com"]);

  renderer.renderReader();
  assert.equal(listeners.size, 1, "rerender removes the old frame listener");
  email.htmlBody = "";
  renderer.renderReader();
  assert.equal(listeners.size, 0, "plain-text messages remove the frame listener");
  email.htmlBody = "<p>HTML again</p>";
  renderer.renderReader();
  assert.equal(listeners.size, 1);
  renderer.dispose();
  renderer.dispose();
  assert.equal(listeners.size, 0, "disposing the renderer removes its listener");
});

test("updateEmailReadState unbolds only the matching list item", () => {
  const email = { id: "message-target", read: false };
  const reader = createReaderElement();
  reader.innerHTML = "reader content must remain mounted";
  const toggles = [];
  const item = {
    dataset: { id: email.id },
    classList: {
      toggle(className, force) {
        toggles.push({ className, force });
      },
    },
  };
  const renderer = createRenderer(createState(email), {
    mailReaderEl: reader,
    mailListEl: {
      querySelectorAll() {
        return [{ dataset: { id: "other-message" } }, item];
      },
    },
  });

  assert.equal(renderer.updateEmailReadState(email.id, true), true);
  assert.deepEqual(toggles, [{ className: "unread", force: false }]);
  assert.equal(reader.innerHTML, "reader content must remain mounted");
  assert.equal(renderer.updateEmailReadState("missing-message", true), false);
});
