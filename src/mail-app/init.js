import { INITIAL_BATCH_SIZE, NEXT_BATCH_SIZE } from "./constants.js";
import { createInitialState } from "./state.js";
import { getMailAppDom } from "./dom.js";
import { createUiController } from "./ui.js";
import { createLayoutController } from "./layout.js";
import { createRenderer } from "./render.js";
import { createSyncController } from "./sync.js";
import {
  clearCachedEmails,
  getEmails,
  hydrateFolderCachesFromStorage,
  saveCachedEmails,
} from "./cache.js";
import {
  getActiveBackendFolderKey,
  getFolderPaginationState,
  resetFolderPagination,
} from "./helpers.js";

export function initializeMailApp({
  invoke,
  listen,
  storage = window.localStorage,
}) {
  const dom = getMailAppDom();

  if (
    !dom.mailBodyEl ||
    !dom.mailListPanelEl ||
    !dom.mailListEl ||
    !dom.mailReaderEl ||
    !dom.searchInputEl ||
    !dom.composeBtnEl ||
    !dom.refreshBtnEl ||
    !dom.spamCheckBtnEl ||
    !dom.markReadBtnEl ||
    !dom.archiveBtnEl
  ) {
    return;
  }

  const state = createInitialState();
  const ui = createUiController(dom);
  const renderer = createRenderer(state, dom);
  const layout = createLayoutController(state, dom);

  if (!invoke) {
    ui.showMessage("Backend invoke bridge is unavailable.", true);
    return;
  }

  const sync = createSyncController(state, renderer, ui, invoke, storage);

  if (listen) {
    listen("app://logged-out", () => {
      state.allowFetch = false;
      state.isAppReady = false;
      sync.stopBootstrapRefresh();
      sync.clearInitialEmptyRetry();
      sync.clearInitialSyncPoll();
      clearCachedEmails(storage);
      state.cachedByFolder.INBOX = [];
      state.cachedByFolder.Sent = [];
      state.cachedByFolder.Trash = [];
      resetFolderPagination(state, "INBOX");
      resetFolderPagination(state, "Sent");
      resetFolderPagination(state, "Trash");
      window.location.replace("/setup.html");
    }).catch(() => {
      // Ignore listener setup failures in non-Tauri contexts.
    });
  }

  dom.folderButtons.forEach((button) => {
    button.addEventListener("click", () => {
      sync.clearInitialEmptyRetry();
      state.initialEmptyRetries = 0;
      state.syncProgressPercent = 0;
      state.activeFolder = button.dataset.folder;
      dom.folderButtons.forEach((otherButton) =>
        otherButton.classList.remove("active"),
      );
      button.classList.add("active");
      renderer.renderList();

      if (state.activeFolder === "INBOX" || state.activeFolder === "Sent") {
        const folderKey = getActiveBackendFolderKey(state);
        if (state.cachedByFolder[folderKey].length === 0) {
          sync.loadMoreEmails(INITIAL_BATCH_SIZE, { reset: true });
        }
      }
    });
  });

  layout.setupLayoutEvents();

  dom.mailListEl.addEventListener("click", (event) => {
    const target = event.target.closest(".mail-item");

    if (!target?.dataset?.id) {
      return;
    }

    state.selectedId = target.dataset.id;
    renderer.renderList();
  });

  dom.searchInputEl.addEventListener("input", () => {
    state.query = dom.searchInputEl.value;
    renderer.renderList();
  });

  dom.mailListEl.addEventListener("scroll", () => {
    const folderKey = getActiveBackendFolderKey(state);
    const pagination = getFolderPaginationState(state, folderKey);

    if (state.isLoadingEmails || !pagination.hasMoreEmails) {
      return;
    }

    const threshold = 40;
    const reachedBottom =
      dom.mailListEl.scrollTop + dom.mailListEl.clientHeight >=
      dom.mailListEl.scrollHeight - threshold;

    if (reachedBottom) {
      sync.loadMoreEmails(NEXT_BATCH_SIZE);
    }
  });

  dom.composeBtnEl.addEventListener("click", () => {
    const draftId = `draft-${Date.now()}`;
    state.cachedByFolder.Sent.unshift({
      id: draftId,
      folder: "Sent",
      senderName: "You",
      emailFrom: "you@company.com",
      subject: "Draft: New message",
      preview:
        "This is a placeholder draft. Wire this to your compose modal later.",
      body: "Draft created from the home UI. Replace this with your backend compose flow.",
      time: "Now",
      starred: false,
      read: true,
    });

    state.activeFolder = "Sent";
    dom.folderButtons.forEach((otherButton) => {
      otherButton.classList.toggle(
        "active",
        otherButton.dataset.folder === "Sent",
      );
    });

    state.selectedId = draftId;
    renderer.renderList();
    saveCachedEmails(state, storage);
    ui.showMessage("Draft created in Sent.");
  });

  dom.refreshBtnEl.addEventListener("click", async () => {
    try {
      const newCount = await sync.refreshActiveMailbox();
      if (newCount > 0) {
        ui.showMessage(
          `Mailbox refreshed. ${newCount} new email${newCount === 1 ? "" : "s"}.`,
        );
      } else {
        ui.showMessage("Mailbox refreshed. No new emails.");
      }
    } catch (error) {
      const message = error?.message || error?.msg || String(error);
      ui.showMessage(`Failed to refresh mailbox: ${message}`, true);
    }
  });

  dom.markReadBtnEl.addEventListener("click", () => {
    const selected = getEmails(state).find(
      (mail) => mail.id === state.selectedId,
    );

    if (!selected) {
      ui.showMessage("Select an email first.", true);
      return;
    }

    selected.read = true;
    renderer.renderList();
    saveCachedEmails(state, storage);
    ui.showMessage("Marked as read.");
  });

  dom.archiveBtnEl.addEventListener("click", () => {
    const selected = getEmails(state).find(
      (mail) => mail.id === state.selectedId,
    );

    if (!selected) {
      ui.showMessage("Select an email first.", true);
      return;
    }

    selected.folder = "Trash";
    state.selectedId = null;
    renderer.renderList();
    saveCachedEmails(state, storage);
    ui.showMessage("Email moved to archive.");
  });

  dom.spamCheckBtnEl.addEventListener("click", async () => {
    const selected = getEmails(state).find(
      (mail) => mail.id === state.selectedId,
    );

    if (!selected) {
      ui.showMessage("Select an email first.", true);
      return;
    }

    try {
      const rating = await invoke("rater", {
        subject: selected.subject,
        emailFrom: selected.emailFrom,
        body: selected.body,
      });

      const score =
        typeof rating?.score === "number" ? rating.score.toFixed(2) : "N/A";
      const label = rating?.label ?? "Unknown";
      ui.showMessage(`Spam rating: ${label} (score: ${score})`);
    } catch (error) {
      ui.showMessage(`Failed to check spam rating: ${error}`, true);
    }
  });

  hydrateFolderCachesFromStorage(state, storage);
  layout.syncLayoutState();

  if (getEmails(state).length > 0) {
    renderer.renderList();
  }

  (async () => {
    console.log("Starts the initial app");
    ui.setSyncUiState(true, "Syncing mailbox...", 0);

    console.log("Checking initial sync status...");
    const isReady = await sync.checkInitStatus();
    if (!isReady) {
      return;
    }

    state.isAppReady = true;
    console.log("Starts the sync process");
    await sync.runInitialSync();

    if (!state.isInitialSyncComplete) {
      return;
    }

    ui.setSyncUiState(false);
  })();
}
