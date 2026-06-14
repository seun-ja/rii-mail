import {
  inboxFolderPopulationListener,
  sentFolderPopulationListener,
  inboxInitialPopulationCompleted,
  sentInitialPopulationCompleted,
} from "./listener.js";
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
import { INITIAL_BATCH_SIZE, NEXT_BATCH_SIZE } from "./constants.js";

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
  state.folderPopulation = {
    inbox: 0,
    sent: 0,
  };
  const ui = createUiController(dom);
  const renderer = createRenderer(state, dom);
  const layout = createLayoutController(state, dom);

  if (!invoke) {
    ui.showMessage("Backend invoke bridge is unavailable.", true);
    return;
  }

  const sync = createSyncController(state, renderer, ui, invoke, storage);
  invoke("inbox_email_populated").catch(console.error);
  invoke("sent_email_populated").catch(console.error);

  invoke("inbox_intial_email_populated").catch(console.error);
  invoke("sent_intial_email_populated").catch(console.error);
  const cleanupListeners = [];

  (async () => {
    const inboxProgress = await inboxFolderPopulationListener((progress) => {
      state.folderPopulation.inbox = progress;
      ui.updateDbPopulation(
        state.folderPopulation.inbox,
        state.folderPopulation.sent,
      );
    }, listen);

    cleanupListeners.push(inboxProgress);

    const sentProgress = await sentFolderPopulationListener((progress) => {
      state.folderPopulation.sent = progress;
      ui.updateDbPopulation(
        state.folderPopulation.inbox,
        state.folderPopulation.sent,
      );
    }, listen);

    cleanupListeners.push(sentProgress);

    const inboxReady = await inboxInitialPopulationCompleted(async () => {
      if (state.isInitialSyncComplete) {
        return;
      }

      state.isInitialSyncComplete = true;

      await sync.loadMoreEmails(INITIAL_BATCH_SIZE, {
        reset: true,
      });

      ui.setSyncUiState(false);
    }, listen);

    cleanupListeners.push(inboxReady);

    const sentReady = await sentInitialPopulationCompleted(() => {
      console.log("Initial Sent mailbox population complete");
    }, listen);

    cleanupListeners.push(sentReady);
  })();

  const haltSyncAndClearCache = () => {
    sync.haltAllSync();
    clearCachedEmails(storage);
    state.cachedByFolder.INBOX = [];
    state.cachedByFolder.Sent = [];
    state.cachedByFolder.Trash = [];
    state.totalEmailsByFolder.INBOX = null;
    state.totalEmailsByFolder.Sent = null;
    state.totalEmailsByFolder.Trash = null;
    resetFolderPagination(state, "INBOX");
    resetFolderPagination(state, "Sent");
    resetFolderPagination(state, "Trash");
  };

  if (listen) {
    listen("app://logged-out", () => {
      state.isAppReady = false;
      haltSyncAndClearCache();
      window.location.replace("/login.html");
    }).catch(() => {
      // Ignore listener setup failures in non-Tauri contexts.
    });
  }

  window.addEventListener("pagehide", () => {
    cleanupListeners.forEach((unlisten) => {
      try {
        unlisten();
      } catch (_) {}
    });

    haltSyncAndClearCache();
  });

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
    dom.composeModalEl.classList.remove("hidden");
  });

  dom.closeComposeBtnEl.addEventListener("click", () => {
    dom.composeModalEl.classList.add("hidden");
  });

  dom.composeModalEl.addEventListener("click", (e) => {
    if (e.target === dom.composeModalEl) {
      dom.composeModalEl.classList.add("hidden");
    }
  });

  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape") {
      dom.composeModalEl.classList.add("hidden");
    }
  });

  const toggleCcBtn = document.getElementById("toggle-cc-btn");
  const toggleBccBtn = document.getElementById("toggle-bcc-btn");

  const ccRow = document.getElementById("compose-cc-row");
  const bccRow = document.getElementById("compose-bcc-row");

  toggleCcBtn?.addEventListener("click", () => {
    ccRow.classList.toggle("hidden");

    if (!ccRow.classList.contains("hidden")) {
      document.getElementById("compose-cc").focus();
    }
  });

  toggleBccBtn?.addEventListener("click", () => {
    bccRow.classList.toggle("hidden");

    if (!bccRow.classList.contains("hidden")) {
      document.getElementById("compose-bcc").focus();
    }
  });

  dom.sendComposeBtnEl.addEventListener("click", async () => {
    const toEmails = dom.composeToInput.value
      .split(/[,;]/)
      .map((e) => e.trim())
      .filter(Boolean);
    const ccEmails = dom.composeCcInput.value
      .split(/[,;]/)
      .map((e) => e.trim())
      .filter(Boolean);
    const bccEmails = dom.composeBccInput.value
      .split(/[,;]/)
      .map((e) => e.trim())
      .filter(Boolean);

    const subject = dom.composeSubjectInput.value.trim();
    const body = dom.composeBodyInput.value.trim();

    if (toEmails.length === 0) {
      ui.showMessage("Recipient email is required.", true);
      return;
    }

    if (!subject || !body) {
      ui.showMessage("Subject and body are required.", true);
      return;
    }

    try {
      ui.showMessage("Sending email...");

      await invoke("send_email", {
        to: toEmails.map((email) => ["", email]),
        cc: ccEmails.map((ccEmail) => ["", ccEmail]),
        bcc: bccEmails.map((bccEmail) => ["", bccEmail]),
        subject,
        body,
        contentType: "text/plain",
      });

      ui.showMessage("Email sent successfully!");

      dom.composeModalEl.classList.add("hidden");

      dom.composeToInput.value = "";
      dom.composeSubjectInput.value = "";
      dom.composeBodyInput.value = "";

      state.activeFolder = "INBOX";
      renderer.renderList();
    } catch (err) {
      console.error(err);
      ui.showMessage(`Error sending email: ${err?.message ?? err}`, true);
    }
  });

  dom.refreshBtnEl.addEventListener("click", async () => {
    try {
      const { newCount, totalEmails } = await sync.refreshActiveMailbox();

      if (typeof totalEmails === "number") {
        const noun = totalEmails === 1 ? "email" : "emails";
        ui.showMessage(`Mailbox refreshed. ${totalEmails} total ${noun}.`);
      } else if (newCount > 0) {
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

  const aiPanel = document.getElementById("ai-assistant-panel");

  document
    .getElementById("open-ai-assistant-btn")
    ?.addEventListener("click", () => {
      aiPanel.classList.remove("hidden");
    });

  document
    .getElementById("close-ai-assistant-btn")
    ?.addEventListener("click", () => {
      aiPanel.classList.add("hidden");
    });

  const generateBtn = document.getElementById("generate-email-btn");

  generateBtn?.addEventListener("click", async () => {
    const prompt = document.getElementById("ai-email-prompt").value;

    if (!prompt.trim()) return;

    const status = document.getElementById("generate-email-status");

    try {
      status.textContent = "Generating draft...";

      const email = await invoke("email_generator", {
        thoughts: prompt,
        context: null,
      });

      if (email.to) {
        document.getElementById("compose-to").value = email.to;
      }

      if (email.cc) {
        document.getElementById("compose-cc").value = email.cc;
      }

      if (email.bcc) {
        document.getElementById("compose-bcc").value = email.bcc;
      }

      document.getElementById("compose-subject").value = email.subject ?? "";

      document.getElementById("compose-body").value = email.body ?? "";

      status.textContent = "Draft inserted into composer";
    } catch (err) {
      status.textContent = "Failed to generate draft";
      console.error(err);
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

      const rawScore = rating?.score;
      const hasScore = typeof rawScore === "number";

      let label = rating?.label ?? "Unknown";
      let labelText = label === "Ham" ? "safe" : label.toLowerCase();

      if (hasScore) {
        const displayScore = rawScore.toFixed(2);

        if (rawScore <= 0.2) {
          ui.showMessage(
            `This email is likely ${labelText} (score: ${displayScore})`,
          );
        } else if (rawScore >= 0.8) {
          ui.showMessage(`This email is ${labelText} (score: ${displayScore})`);
        } else {
          ui.showMessage(`I think it's ${labelText} (score: ${displayScore})`);
        }
      } else {
        ui.showMessage(`Could not determine email status (score: N/A)`);
      }
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

    state.isAppReady = true;

    await sync.runInitialSync();
    ui.setSyncUiState(false);
  })();
}
