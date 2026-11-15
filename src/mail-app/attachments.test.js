import test from "node:test";
import assert from "node:assert/strict";

import {
  createAttachmentPreview,
  decodeBase64Attachment,
  formatAttachmentSize,
  getPreviewableAttachments,
} from "./attachments.js";

test("getPreviewableAttachments rejects stale metadata without server part IDs", () => {
  const email = {
    attachments: [
      { id: 0, fileName: "legacy.pdf" },
      { id: 1, partId: "2.1", fileName: "current.pdf" },
      { id: 2, partId: "0", fileName: "invalid.pdf" },
    ],
  };

  assert.deepEqual(getPreviewableAttachments(email), [email.attachments[1]]);
  assert.deepEqual(getPreviewableAttachments({}), []);
});

test("decodeBase64Attachment decodes attachment responses", () => {
  assert.deepEqual([...decodeBase64Attachment("AQIDBA==")], [1, 2, 3, 4]);
});

test("createAttachmentPreview creates and revokes image object URLs", () => {
  const events = [];
  const urlApi = {
    createObjectURL(blob) {
      events.push(["create", blob.type, blob.size]);
      return "blob:preview";
    },
    revokeObjectURL(url) {
      events.push(["revoke", url]);
    },
  };
  const preview = createAttachmentPreview(
    {
      contentType: "image/png",
      base64Data: "AQIDBA==",
    },
    { urlApi },
  );

  assert.equal(preview.kind, "image");
  assert.equal(preview.objectUrl, "blob:preview");
  preview.dispose();
  preview.dispose();
  assert.deepEqual(events, [
    ["create", "image/png", 4],
    ["revoke", "blob:preview"],
  ]);
});

test("createAttachmentPreview decodes text and identifies unsupported files", () => {
  const text = createAttachmentPreview({
    contentType: "text/plain",
    base64Data: "SGVsbG8=",
  });
  const unsupported = createAttachmentPreview({
    contentType: "application/zip",
    base64Data: "AQI=",
  });

  assert.equal(text.kind, "text");
  assert.equal(text.text, "Hello");
  assert.equal(unsupported.kind, "unsupported");
  assert.equal(unsupported.objectUrl, null);
});

test("formatAttachmentSize formats byte counts for the reader", () => {
  assert.equal(formatAttachmentSize(12), "12 B");
  assert.equal(formatAttachmentSize(1536), "1.5 KB");
  assert.equal(formatAttachmentSize(5 * 1024 * 1024), "5.0 MB");
  assert.equal(formatAttachmentSize(-1), "");
});

