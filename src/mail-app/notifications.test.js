import test from "node:test";
import assert from "node:assert/strict";

import { createNotificationController, normalizeBackendEmail } from "./notifications.js";
import { createInitialState } from "./state.js";
import { createSyncController } from "./sync.js";
import { EMAIL_CACHE_KEY } from "./constants.js";

function deferred() {
  let resolve;
  let reject;
  const promise = new Promise((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

function email(id, extra = {}) {
  return {
    id: `db-INBOX-uid-${id}`,
    folder: "inbox",
    senderName: "Sender",
    emailFrom: "sender@example.com",
    subject: `Message ${id}`,
    preview: "Preview",
    body: "Body",
    textBody: "Full text",
    htmlBody: "<p>Full HTML</p>",
    cc: [["Copy", "copy@example.com"]],
    read: false,
    starred: false,
    time: "Oct 31 12:00",
    ...extra,
  };
}

function harness({ invoke, listen } = {}) {
  const state = createInitialState();
  state.isAppReady = true;
  const values = new Map([["riimail.provider", "custom"]]);
  const storage = {
    getItem: (key) => values.get(key) ?? null,
    setItem: (key, value) => values.set(key, value),
  };
  const buttons = ["INBOX", "Sent", "starred", "Trash"].map((folder) => ({
    dataset: { folder },
    active: folder === "Sent",
    classList: {
      toggle(_name, active) { buttons.find((button) => button.dataset.folder === folder).active = active; },
    },
  }));
  const dom = { searchInputEl: { value: "hidden" }, folderButtons: buttons };
  const calls = [];
  const messages = [];
  const renders = [];
  const handlers = new Map();
  let unlistens = 0;
  const queue = [];
  let controller;
  const renderer = {
    renderList() {
      controller.reconcileCache();
      renders.push(state.selectedId);
      const selected = state.cachedByFolder.INBOX.find((mail) => mail.id === state.selectedId);
      if (selected) selected.read = true;
    },
  };
  controller = createNotificationController({
    state, dom, renderer, storage,
    ui: { showMessage: (...args) => messages.push(args) },
    invoke: async (command, args) => {
      calls.push({ command, args });
      return invoke ? invoke(command, args) : queue.shift() ?? null;
    },
    listen: listen ?? (async (name, callback) => {
      handlers.set(name, callback);
      return () => { unlistens += 1; handlers.delete(name); };
    }),
  });
  return {
    state, storage, dom, calls, messages, renders, handlers, queue, controller, renderer,
    get unlistens() { return unlistens; },
  };
}

async function click(h, payload) {
  h.handlers.get("app://notification-clicked")({ payload });
  await new Promise(setImmediate);
}

test("click opens the exact older message, not the newest or event payload, and clears visibility filters", async () => {
  const h = harness();
  h.state.activeFolder = "starred";
  h.state.query = "hidden";
  const newest = normalizeBackendEmail(email(100));
  const target = normalizeBackendEmail(email(40, { subject: "Stale subject" }));
  h.state.cachedByFolder.INBOX = [newest, target];
  h.state.selectedId = newest.id;
  await h.controller.initialSyncComplete();
  h.queue.push(email(40, { subject: "Exact clicked message" }));

  await click(h, { id: newest.id });

  assert.equal(h.state.selectedId, target.id);
  assert.equal(h.state.activeFolder, "INBOX");
  assert.equal(h.state.query, "");
  assert.equal(h.dom.searchInputEl.value, "");
  assert.deepEqual(h.dom.folderButtons.filter((button) => button.active).map((button) => button.dataset.folder), ["INBOX"]);
  assert.equal(h.state.cachedByFolder.INBOX.length, 2);
  assert.equal(h.state.cachedByFolder.INBOX[1].subject, "Exact clicked message");
  assert.equal(h.renders.at(-1), target.id);
  const saved = JSON.parse(h.storage.getItem(EMAIL_CACHE_KEY));
  assert.equal(saved[1].read, true);
  assert.ok(h.calls.every(({ command, args }) => command === "take_notification_email" && args === undefined));
});

test("cache miss takes the local exact record, preserves all FrontendEmail fields and pagination", async () => {
  const h = harness();
  h.state.cachedByFolder.INBOX = [normalizeBackendEmail(email(100))];
  Object.assign(h.state.paginationByFolder.INBOX, { nextOffset: 20, hasMoreEmails: true, lastFetchSignature: "page", lastFetchAt: 42 });
  h.state.totalEmailsByFolder.INBOX = 200;
  const pagination = { ...h.state.paginationByFolder.INBOX };
  const target = email(3);
  h.queue.push(target);

  await h.controller.initialSyncComplete();

  assert.equal(h.state.selectedId, target.id);
  assert.deepEqual(h.state.cachedByFolder.INBOX[1], { ...target, folder: "INBOX", read: true });
  assert.deepEqual(h.state.paginationByFolder.INBOX, pagination);
  assert.equal(h.state.totalEmailsByFolder.INBOX, 200);
  assert.deepEqual(h.calls.map(({ command }) => command), ["take_notification_email", "take_notification_email"]);
});

test("pending at initialization and early events wait until initial sync finishes replacing its cache", async () => {
  const h = harness();
  h.queue.push(email(7));
  h.handlers.get("app://notification-clicked")({ payload: null });
  await h.controller.checkPending();
  assert.equal(h.calls.length, 0);
  h.state.cachedByFolder.INBOX = [normalizeBackendEmail(email(100))];

  await h.controller.initialSyncComplete();

  assert.equal(h.state.selectedId, email(7).id);
  assert.equal(h.state.cachedByFolder.INBOX.length, 2);
});

test("initial pending check works without a click event; dismissed/no target leaves UI alone", async () => {
  const h = harness();
  h.state.activeFolder = "Sent";
  h.state.query = "keep query";
  h.state.selectedId = "existing";
  await h.controller.initialSyncComplete();
  await click(h, { id: "not-a-target" });
  assert.equal(h.state.activeFolder, "Sent");
  assert.equal(h.state.query, "keep query");
  assert.equal(h.state.selectedId, "existing");
  assert.equal(h.renders.length, 0);
  assert.equal(h.storage.getItem(EMAIL_CACHE_KEY), null);

  const pending = harness();
  pending.queue.push(email(8));
  await pending.controller.initialSyncComplete();
  assert.equal(pending.state.selectedId, email(8).id);
});

test("serializes clicks during an in-flight take and drains all queued targets", async () => {
  const first = deferred();
  let active = 0;
  let maxActive = 0;
  let count = 0;
  const h = harness({ invoke: async () => {
    active += 1;
    maxActive = Math.max(maxActive, active);
    const result = ++count === 1 ? await first.promise : count === 2 ? email(2) : null;
    active -= 1;
    return result;
  } });
  const startup = h.controller.initialSyncComplete();
  await new Promise(setImmediate);
  h.handlers.get("app://notification-clicked")({ payload: "ignored" });
  h.handlers.get("app://notification-clicked")({ payload: "ignored" });
  first.resolve(email(1));
  await startup;
  assert.equal(maxActive, 1);
  assert.equal(count, 3);
  assert.deepEqual(h.renders, [email(1).id, email(2).id]);
  assert.equal(h.state.selectedId, email(2).id);
});

test("visible command errors do not change selection or spin; next click can retry", async () => {
  let fail = true;
  const h = harness({ invoke: async () => {
    if (fail) throw new Error("Local database unavailable");
    return null;
  } });
  h.state.selectedId = "existing";
  await h.controller.initialSyncComplete();
  assert.equal(h.calls.length, 1);
  assert.match(h.messages[0][0], /Local database unavailable/);
  assert.equal(h.messages[0][1], true);
  assert.equal(h.state.selectedId, "existing");
  fail = false;
  await click(h);
  assert.equal(h.calls.length, 2);
});

test("listener registration errors are visible but the startup pending check still works", async () => {
  const h = harness({ listen: async () => { throw new Error("Listen failed"); } });
  h.queue.push(email(9));
  await h.controller.initialSyncComplete();
  assert.match(h.messages[0][0], /Listen failed/);
  assert.equal(h.state.selectedId, email(9).id);
});

test("pagehide/logout disposal ignores in-flight results and errors, removes listeners, and prevents restart", async () => {
  for (const reject of [false, true]) {
    const result = deferred();
    const h = harness({ invoke: () => result.promise });
    const startup = h.controller.initialSyncComplete();
    await new Promise(setImmediate);
    h.controller.dispose();
    h.controller.dispose();
    h.state.isAppReady = false;
    h.state.allowFetch = false;
    if (reject) result.reject(new Error("Late failure"));
    else result.resolve(email(12));
    await startup;
    await h.controller.initialSyncComplete();
    await h.controller.checkPending();
    assert.equal(h.calls.length, 1);
    assert.equal(h.unlistens, 1);
    assert.equal(h.state.selectedId, null);
    assert.equal(h.state.cachedByFolder.INBOX.length, 0);
    assert.equal(h.renders.length, 0);
    assert.equal(h.messages.length, 0);
    assert.equal(h.storage.getItem(EMAIL_CACHE_KEY), null);
  }
});

test("logout flags alone reject a stale result", async () => {
  const result = deferred();
  const h = harness({ invoke: () => result.promise });
  const startup = h.controller.initialSyncComplete();
  await new Promise(setImmediate);
  h.state.allowFetch = false;
  result.resolve(email(13));
  await startup;
  assert.equal(h.state.selectedId, null);
  assert.equal(h.renders.length, 0);
});

test("listener resolving after disposal is immediately cleaned up", async () => {
  const registration = deferred();
  let cleanups = 0;
  const h = harness({ listen: () => registration.promise });
  h.controller.dispose();
  registration.resolve(() => { cleanups += 1; });
  await h.controller.initialSyncComplete();
  assert.equal(cleanups, 1);
  assert.equal(h.calls.length, 0);
});

test("background page replacement retains target and subsequent pagination uses fetched rows, deduplicating the target", async () => {
  const h = harness();
  h.queue.push(email(3));
  await h.controller.initialSyncComplete();
  h.state.cachedByFolder.INBOX = [normalizeBackendEmail(email(100))];
  h.state.paginationByFolder.INBOX.nextOffset = 1;
  h.renderer.renderList();
  assert.equal(h.state.selectedId, email(3).id);
  assert.equal(h.state.cachedByFolder.INBOX.length, 2);
  assert.equal(h.state.paginationByFolder.INBOX.nextOffset, 1);

  const calls = [];
  const sync = createSyncController(h.state, h.renderer, { showMessage() {} }, async (command, args) => {
    calls.push({ command, args });
    return { emails: [normalizeBackendEmail(email(3)), normalizeBackendEmail(email(2))] };
  }, h.storage);
  await sync.loadMoreEmails(2);
  assert.equal(calls[0].args.minRange, 1);
  assert.equal(calls[0].args.maxRange, 3);
  assert.equal(h.state.paginationByFolder.INBOX.nextOffset, 3);
  assert.equal(h.state.paginationByFolder.INBOX.hasMoreEmails, true);
  assert.equal(h.state.cachedByFolder.INBOX.filter((mail) => mail.id === email(3).id).length, 1);
  assert.equal(h.state.selectedId, email(3).id);
});

test("malformed target is reported without choosing a different message", async () => {
  const h = harness();
  h.queue.push({ subject: "Missing ID" });
  await h.controller.initialSyncComplete();
  assert.match(h.messages[0][0], /valid message ID/);
  assert.equal(h.state.selectedId, null);
  assert.equal(h.state.cachedByFolder.INBOX.length, 0);
});
