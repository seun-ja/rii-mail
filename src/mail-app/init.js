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
  createForwardDraft,
  createReplyDraft,
  getActiveBackendFolderKey,
  getFolderPaginationState,
  isMailListNearBottom,
  resetFolderPagination,
} from "./helpers.js";
import { INITIAL_BATCH_SIZE, NEXT_BATCH_SIZE } from "./constants.js";
import {
  formatSpamRatingError,
  formatSpamRatingResponse,
} from "./spam-rating.js";
import { updateAiAssistantAvailability } from "./ai-assistant.js";

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

  void updateAiAssistantAvailability({
    invoke,
    button: dom.openAiAssistantBtnEl,
    panel: dom.aiAssistantPanelEl,
  });

  const sync = createSyncController(state, renderer, ui, invoke, storage);
  const cleanupListeners = [];

  void (async () => {
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
      // The modal may already be dismissed while the first IMAP batch is still
      // being imported. This event is the authoritative signal that its local
      // database page can now be read, so it must not be skipped in that case.
      state.isInitialInboxDbPopulated = true;

      await sync.loadMoreEmails(INITIAL_BATCH_SIZE, {
        reset: true,
        retryWhenBusy: true,
      });
      await loadMoreEmailsIfListEndVisible();

      ui.setSyncUiState(false);
    }, listen);

    cleanupListeners.push(inboxReady);

    const sentReady = await sentInitialPopulationCompleted(() => {
      console.log("Initial Sent mailbox population complete");
    }, listen);

    cleanupListeners.push(sentReady);

    // The native completion bridges consume one-shot receivers. Register every
    // frontend listener first so a fast initial import cannot emit its only
    // readiness event before the UI is listening.
    invoke("inbox_email_populated").catch(console.error);
    invoke("sent_email_populated").catch(console.error);
    invoke("inbox_intial_email_populated").catch(console.error);
    invoke("sent_intial_email_populated").catch(console.error);
  })().catch((error) => {
    console.error("Failed to register mailbox sync listeners", error);
  });

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

  async function loadMoreEmailsIfListEndVisible(batchSize = NEXT_BATCH_SIZE) {
    if (state.activeFolder === "starred") {
      return;
    }

    const requestedFolderKey = getActiveBackendFolderKey(state);
    const maxAutoLoads = 10;

    for (let attempt = 0; attempt < maxAutoLoads; attempt += 1) {
      if (getActiveBackendFolderKey(state) !== requestedFolderKey) {
        return;
      }

      const pagination = getFolderPaginationState(state, requestedFolderKey);
      if (
        state.isLoadingEmails ||
        !pagination?.hasMoreEmails ||
        !isMailListNearBottom(dom.mailListEl)
      ) {
        return;
      }

      const previousOffset = pagination.nextOffset;
      await sync.loadMoreEmails(batchSize);

      if (
        getActiveBackendFolderKey(state) !== requestedFolderKey ||
        getFolderPaginationState(state, requestedFolderKey)?.nextOffset ===
          previousOffset
      ) {
        return;
      }
    }
  }

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

      if (state.activeFolder !== "INBOX" && state.activeFolder !== "Sent") {
        return;
      }

      const folderKey = getActiveBackendFolderKey(state);
      if (state.cachedByFolder[folderKey].length === 0) {
        void sync
          .loadMoreEmails(INITIAL_BATCH_SIZE, { reset: true })
          .then(() => loadMoreEmailsIfListEndVisible());
        return;
      }

      void loadMoreEmailsIfListEndVisible();
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

    if (isMailListNearBottom(dom.mailListEl)) {
      void loadMoreEmailsIfListEndVisible();
    }
  });

  function openComposerWithDraft(draft, title) {
    const cc = String(draft.cc ?? "").trim();
    const bcc = String(draft.bcc ?? "").trim();

    document.getElementById("compose-title").textContent = title;
    dom.composeToInput.value = draft.to ?? "";
    dom.composeCcInput.value = cc;
    dom.composeBccInput.value = bcc;
    dom.composeSubjectInput.value = draft.subject ?? "";
    dom.composeBodyInput.value = draft.body ?? "";
    document
      .getElementById("compose-cc-row")
      ?.classList.toggle("hidden", !cc);
    document
      .getElementById("compose-bcc-row")
      ?.classList.toggle("hidden", !bcc);
    dom.composeModalEl.classList.remove("hidden");

    const inputToFocus = draft.to ? dom.composeBodyInput : dom.composeToInput;
    inputToFocus.focus();

    if (inputToFocus === dom.composeBodyInput) {
      inputToFocus.setSelectionRange(0, 0);
    }
  }

  dom.mailReaderEl.addEventListener("click", (event) => {
    const action = event.target.closest("[data-reader-action]")?.dataset
      .readerAction;

    if (!action) {
      return;
    }

    const selected = getEmails(state).find(
      (mail) => mail.id === state.selectedId,
    );

    if (!selected) {
      return;
    }

    if (action === "reply") {
      openComposerWithDraft(createReplyDraft(selected), "Reply");
    } else if (action === "forward") {
      openComposerWithDraft(createForwardDraft(selected), "Forward");
    }
  });

  dom.composeBtnEl.addEventListener("click", () => {
    openComposerWithDraft(
      { to: "", cc: "", bcc: "", subject: "", body: "" },
      "New Message",
    );
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
      dom.composeCcInput.value = "";
      dom.composeBccInput.value = "";
      dom.composeSubjectInput.value = "";
      dom.composeBodyInput.value = "";
      ccRow?.classList.add("hidden");
      bccRow?.classList.add("hidden");

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

  const aiPanel = dom.aiAssistantPanelEl;

  dom.openAiAssistantBtnEl?.addEventListener("click", () => {
    aiPanel?.classList.remove("hidden");
  });

  document
    .getElementById("close-ai-assistant-btn")
    ?.addEventListener("click", () => {
      aiPanel?.classList.add("hidden");
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

      dom.composeToInput.value = email.to ?? "";
      dom.composeCcInput.value = email.cc ?? "";
      dom.composeBccInput.value = email.bcc ?? "";
      ccRow?.classList.toggle("hidden", !dom.composeCcInput.value.trim());
      bccRow?.classList.toggle("hidden", !dom.composeBccInput.value.trim());

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

      ui.showMessage(formatSpamRatingResponse(rating));
    } catch (error) {
      ui.showMessage(formatSpamRatingError(error), true);
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

    try {
      await sync.runInitialSync();
    } catch (error) {
      console.error("Initial mailbox sync failed", error);
      ui.showMessage("Unable to sync mailbox. You can retry with Refresh.", true);
    } finally {
      ui.setSyncUiState(false);
    }
  })();
}
