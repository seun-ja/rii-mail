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

test("renderReader shows escaped Cc recipients with the reader actions", () => {
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
  assert.match(mailReaderEl.innerHTML, /class="reader-meta-item reader-meta-cc"/);
  assert.match(
    mailReaderEl.innerHTML,
    /Bob &amp; Co &lt;bob@example\.com&gt;, carol@example\.com/,
  );
  assert.equal(email.read, true);
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
