import { saveCachedEmails } from "./cache.js";

// FrontendEmail already uses the renderer's camelCase fields. Notification
// targets are exact local inbox records, regardless of their provider labels.
export function normalizeBackendEmail(email) {
  if (!email || typeof email.id !== "string" || !email.id.trim()) {
    throw new Error("Notification email has no valid message ID.");
  }
  return { ...email, folder: "INBOX" };
}

export function createNotificationController({
  state,
  dom,
  renderer,
  ui,
  invoke,
  listen,
  storage,
}) {
  let disposed = false;
  let ready = false;
  let pending = false;
  let draining = null;
  let unlisten = null;
  let openedEmail = null;

  function isActive() {
    return !disposed && state.isAppReady && state.allowFetch;
  }

  function reportError(error) {
    if (!disposed) {
      ui.showMessage(
        `Unable to open notification email: ${error?.message || error?.msg || String(error)}`,
        true,
      );
    }
  }

  function reconcileCache() {
    if (!openedEmail || !isActive()) return;
    const inbox = state.cachedByFolder.INBOX;
    if (!inbox.some((email) => email.id === openedEmail.id)) {
      // This is an out-of-page record, not a fetched page: never advance the
      // backend offset or change hasMoreEmails based on its presence.
      inbox.push(openedEmail);
      saveCachedEmails(state, storage);
    }
  }

  function openEmail(email) {
    openedEmail = normalizeBackendEmail(email);
    const inbox = state.cachedByFolder.INBOX;
    const index = inbox.findIndex((cached) => cached.id === openedEmail.id);
    if (index === -1) {
      inbox.push(openedEmail);
    } else {
      openedEmail = { ...inbox[index], ...openedEmail };
      inbox[index] = openedEmail;
    }
    state.activeFolder = "INBOX";
    state.query = "";
    dom.searchInputEl.value = "";
    dom.folderButtons.forEach((button) => {
      button.classList.toggle("active", button.dataset.folder === "INBOX");
    });
    state.selectedId = openedEmail.id;
    // renderList also renders the reader and updates counts.
    renderer.renderList();
    saveCachedEmails(state, storage);
  }

  async function drain() {
    while (ready && isActive()) {
      pending = false;
      let email;
      try {
        email = await invoke("take_notification_email");
        if (!isActive()) return;
        if (email !== null) openEmail(email);
      } catch (error) {
        if (isActive()) reportError(error);
        // A subsequent click can retry, but a failing command must not spin.
        if (!pending) return;
        continue;
      }
      // Consume again after a target: another click may have queued while the
      // previous command was running, even before its event reaches the UI.
      if (email === null && !pending) return;
    }
  }

  function checkPending() {
    if (disposed) return Promise.resolve();
    pending = true;
    if (!ready || !isActive()) return Promise.resolve();
    if (!draining) {
      draining = drain().finally(() => {
        draining = null;
        if (pending && ready && isActive()) void checkPending();
      });
    }
    return draining;
  }

  // Register immediately, before initial sync. No event payload is a target.
  const listenerReady = (async () => {
    if (!listen) return;
    try {
      const cleanup = await listen("app://notification-clicked", () => {
        void checkPending();
      });
      if (disposed) cleanup();
      else unlisten = cleanup;
    } catch (error) {
      reportError(error);
    }
  })();

  async function initialSyncComplete() {
    await listenerReady;
    if (disposed) return;
    ready = true;
    await checkPending();
  }

  function dispose() {
    disposed = true;
    pending = false;
    openedEmail = null;
    if (unlisten) {
      try {
        unlisten();
      } catch (_) {}
      unlisten = null;
    }
  }

  return { checkPending, initialSyncComplete, reconcileCache, dispose };
}
