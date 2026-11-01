import test from "node:test";
import assert from "node:assert/strict";

import { emailsUpdatedListener } from "./listener.js";

test("emails-updated listener forwards the event payload and cleanup", async () => {
  const payload = {
    mailbox: "INBOX",
    provider: "custom",
    newEmailsCount: 2,
  };
  let eventName;
  let handler;
  let cleanedUp = false;
  const callbackPayloads = [];

  const cleanup = await emailsUpdatedListener(
    (eventPayload) => callbackPayloads.push(eventPayload),
    async (name, callback) => {
      eventName = name;
      handler = callback;
      return () => {
        cleanedUp = true;
      };
    },
  );

  handler({ payload });
  cleanup();

  assert.equal(eventName, "emails-updated");
  assert.deepEqual(callbackPayloads, [payload]);
  assert.equal(cleanedUp, true);
});
