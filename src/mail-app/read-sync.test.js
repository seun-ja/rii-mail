import test from "node:test";
import assert from "node:assert/strict";

import { createReadSyncController } from "./read-sync.js";

function createHarness({ invoke = async () => {}, retryDelaysMs = [] } = {}) {
  const email = {
    id: "db-INBOX-uid-42",
    folder: "INBOX",
    read: false,
  };
  const state = {
    cachedByFolder: { INBOX: [email], Sent: [], Trash: [] },
  };
  const updates = [];
  const messages = [];
  const storage = {
    value: null,
    setItem(_key, value) {
      this.value = value;
    },
  };
  const controller = createReadSyncController({
    state,
    renderer: {
      updateEmailReadState(id, read) {
        updates.push({ id, read });
      },
    },
    ui: {
      showMessage(message, isError) {
        messages.push({ message, isError });
      },
    },
    invoke,
    storage,
    retryDelaysMs,
    delay: async () => {},
    logger: { error() {} },
  });

  return { controller, email, messages, storage, updates };
}

test("markAsRead updates local state immediately while backend sync continues", async () => {
  let finishSync;
  const backendSync = new Promise((resolve) => {
    finishSync = resolve;
  });
  const calls = [];
  const h = createHarness({
    invoke(command, args) {
      calls.push({ command, args });
      return backendSync;
    },
  });

  assert.equal(h.controller.markAsRead(h.email), true);
  assert.equal(h.email.read, true);
  assert.deepEqual(h.updates, [{ id: h.email.id, read: true }]);
  assert.equal(JSON.parse(h.storage.value)[0].read, true);
  assert.deepEqual(calls, [
    {
      command: "set_email_read",
      args: { uid: 42, mailbox: "INBOX", read: true },
    },
  ]);

  finishSync();
  await h.controller.whenIdle(h.email.id);
  assert.deepEqual(h.messages, []);
});

test("markAsRead deduplicates repeat interactions for an optimistically read email", async () => {
  let calls = 0;
  const h = createHarness({
    invoke: async () => {
      calls += 1;
    },
  });

  assert.equal(h.controller.markAsRead(h.email), true);
  assert.equal(h.controller.markAsRead(h.email), false);
  await h.controller.whenIdle(h.email.id);
  assert.equal(calls, 1);
});

test("applyServerUpdates patches read changes from other devices", () => {
  const h = createHarness();

  assert.equal(
    h.controller.applyServerUpdates("INBOX", [{ uid: 42, read: true }]),
    1,
  );
  assert.equal(h.email.read, true);
  assert.deepEqual(h.updates, [{ id: h.email.id, read: true }]);

  assert.equal(
    h.controller.applyServerUpdates("INBOX", [{ uid: 42, read: false }]),
    1,
  );
  assert.equal(h.email.read, false);
  assert.deepEqual(h.updates[1], { id: h.email.id, read: false });
});

test("reconcileEmails preserves optimistic read state during a stale refresh", async () => {
  const h = createHarness();

  h.controller.markAsRead(h.email);
  const [refreshed] = h.controller.reconcileEmails([
    { ...h.email, read: false, subject: "Updated subject" },
  ]);
  await h.controller.whenIdle(h.email.id);

  assert.equal(refreshed.read, true);
  assert.equal(refreshed.subject, "Updated subject");
  assert.equal(
    h.controller.reconcileEmails([{ ...h.email, read: false }])[0].read,
    false,
    "server state wins after the local write finishes",
  );
});

test("read sync retries transient failures before reporting the backend message", async () => {
  let calls = 0;
  const h = createHarness({
    retryDelaysMs: [0, 0],
    invoke: async () => {
      calls += 1;
      throw { kind: "Imap", message: "connection closed" };
    },
  });

  h.controller.markAsRead(h.email);
  await h.controller.whenIdle(h.email.id);

  assert.equal(calls, 3);
  assert.equal(h.email.read, true, "a sync failure must not re-bold the email");
  assert.deepEqual(h.messages, [
    {
      message:
        "Email was marked as read locally, but server synchronization failed: connection closed",
      isError: true,
    },
  ]);
});
