import { getErrorMessage } from "../shared/errors.js";
import { saveCachedEmails } from "./cache.js";

const DEFAULT_RETRY_DELAYS_MS = [500, 1500];

function wait(delayMs) {
  return new Promise((resolve) => setTimeout(resolve, delayMs));
}

function getServerLocation(email) {
  const uid = Number(email?.id?.match(/-uid-(\d+)$/)?.[1]);
  if (!Number.isSafeInteger(uid) || uid <= 0) {
    throw new Error("This email cannot be synchronized with the server.");
  }

  const folder = String(email.folder || "").toLowerCase();
  const mailbox =
    folder === "sent" ? "Sent" : folder === "spam" ? "Spam" : "INBOX";

  return { uid, mailbox };
}

export function createReadSyncController({
  state,
  renderer,
  ui,
  invoke,
  storage,
  isDisposed = () => false,
  retryDelaysMs = DEFAULT_RETRY_DELAYS_MS,
  delay = wait,
  logger = console,
}) {
  const inFlight = new Map();
  const locallyReadIds = new Set();

  async function sendToBackend(email) {
    const { uid, mailbox } = getServerLocation(email);
    let lastError;

    for (let attempt = 0; attempt <= retryDelaysMs.length; attempt += 1) {
      try {
        await invoke("set_email_read", { uid, mailbox, read: true });
        return;
      } catch (error) {
        lastError = error;
        const retryDelay = retryDelaysMs[attempt];
        if (retryDelay === undefined || isDisposed()) break;
        await delay(retryDelay);
      }
    }

    throw lastError;
  }

  function synchronizeInBackground(email) {
    const existing = inFlight.get(email.id);
    if (existing) return existing;

    const request = sendToBackend(email)
      .catch((error) => {
        if (!isDisposed()) {
          logger.error("Could not synchronize email read state", error);
          ui.showMessage(
            `Email was marked as read locally, but server synchronization failed: ${getErrorMessage(error)}`,
            true,
          );
        }
      })
      .finally(() => {
        inFlight.delete(email.id);
        locallyReadIds.delete(email.id);
      });

    inFlight.set(email.id, request);
    return request;
  }

  function reconcileEmails(emails) {
    return emails.map((email) =>
      locallyReadIds.has(email.id) && !email.read
        ? { ...email, read: true }
        : email,
    );
  }

  function applyServerUpdates(mailbox, updates) {
    const emails = state.cachedByFolder[mailbox];
    if (!Array.isArray(emails) || !Array.isArray(updates)) return 0;

    let changed = 0;
    for (const update of updates) {
      if (!Number.isSafeInteger(update?.uid) || typeof update?.read !== "boolean") {
        continue;
      }

      const email = emails.find((candidate) =>
        candidate.id?.endsWith(`-uid-${update.uid}`),
      );
      if (!email) continue;

      const [reconciled] = reconcileEmails([{ ...email, read: update.read }]);
      if (reconciled.read === email.read) continue;

      email.read = reconciled.read;
      renderer.updateEmailReadState(email.id, email.read);
      changed += 1;
    }

    if (changed > 0) saveCachedEmails(state, storage);
    return changed;
  }

  function markAsRead(email) {
    if (!email || email.read) return false;

    // Update the local model and only the affected list item. A full list render
    // would recreate the reader iframe and make the opened email visibly reload.
    email.read = true;
    locallyReadIds.add(email.id);
    renderer.updateEmailReadState(email.id, true);
    saveCachedEmails(state, storage);

    void synchronizeInBackground(email);
    return true;
  }

  return {
    applyServerUpdates,
    markAsRead,
    reconcileEmails,
    whenIdle(emailId) {
      return inFlight.get(emailId) ?? Promise.resolve();
    },
  };
}
