import {
  AUTO_BOOTSTRAP_REFRESH_INTERVAL_MS,
  AUTO_BOOTSTRAP_REFRESH_MAX,
  EMAIL_CACHE_KEY,
  INITIAL_BATCH_SIZE,
  INITIAL_EMPTY_RETRY_DELAY_MS,
  INITIAL_EMPTY_RETRY_MAX,
} from "./constants.js";
import { getEmails, saveCachedEmails } from "./cache.js";
import {
  delay,
  getActiveBackendFolderKey,
  getActiveMailboxLiteral,
  getFolderPaginationState,
  resetFolderPagination,
} from "./helpers.js";
import { getProviderLiteral } from "./provider.js";

export function createSyncController(
  state,
  renderer,
  ui,
  invoke,
  storage = window.localStorage,
) {
  function stopBootstrapRefresh() {
    if (state.bootstrapRefreshTimer) {
      clearInterval(state.bootstrapRefreshTimer);
      state.bootstrapRefreshTimer = null;
    }
  }

  function clearInitialEmptyRetry() {
    if (state.initialEmptyRetryTimer) {
      clearTimeout(state.initialEmptyRetryTimer);
      state.initialEmptyRetryTimer = null;
    }
  }

  function clearInitialSyncPoll() {
    if (state.initialSyncPollTimer) {
      clearTimeout(state.initialSyncPollTimer);
      state.initialSyncPollTimer = null;
    }
  }

  function startBootstrapRefresh() {
    stopBootstrapRefresh();
    state.bootstrapRefreshAttempts = 0;

    state.bootstrapRefreshTimer = setInterval(() => {
      if (
        getEmails(state).length > 0 ||
        state.bootstrapRefreshAttempts >= AUTO_BOOTSTRAP_REFRESH_MAX
      ) {
        stopBootstrapRefresh();
        return;
      }

      if (!state.isLoadingEmails) {
        loadMoreEmails(INITIAL_BATCH_SIZE, { reset: true });
      }

      state.bootstrapRefreshAttempts += 1;
    }, AUTO_BOOTSTRAP_REFRESH_INTERVAL_MS);
  }

  // TODO: should take it a mailbosx argument to support manual refresh of non-active folders?
  async function loadMoreEmails(
    batchSize,
    { reset = false, preserveEmptyRetryState = false } = {},
  ) {
    if (!state.allowFetch || !state.isAppReady || state.isLoadingEmails) {
      return;
    }

    const folderKey = getActiveBackendFolderKey(state);
    const pagination = getFolderPaginationState(state, folderKey);

    if (!reset && !pagination.hasMoreEmails) {
      return;
    }

    if (reset) {
      const hasExistingData = state.cachedByFolder[folderKey].length > 0;
      if (!hasExistingData) {
        state.selectedId = null;
      }
      resetFolderPagination(state, folderKey);

      if (!preserveEmptyRetryState) {
        state.initialEmptyRetries = 0;
        clearInitialEmptyRetry();
      }
    }

    const minRange = pagination.nextOffset;
    const maxRange = minRange + batchSize;
    const fetchSignature = `${state.activeFolder}:${minRange}:${maxRange}`;
    const now = Date.now();

    if (
      pagination.lastFetchSignature === fetchSignature &&
      now - pagination.lastFetchAt < 1200
    ) {
      return;
    }

    pagination.lastFetchSignature = fetchSignature;
    pagination.lastFetchAt = now;

    state.isLoadingEmails = true;
    renderer.renderList();

    try {
      const mailbox = getActiveMailboxLiteral(state);
      const provider = getProviderLiteral(storage);

      const fetched = await invoke("fetch_emails_handler", {
        minRange,
        maxRange,
        mailbox,
        provider,
      });

      const page = Array.isArray(fetched) ? fetched : [];

      if (page.length > 0) {
        stopBootstrapRefresh();
        clearInitialEmptyRetry();
        state.initialEmptyRetries = 0;
        state.syncProgressPercent = 100;
      }

      if (reset) {
        state.cachedByFolder[folderKey] = page;
      } else {
        const existingIds = new Set(
          state.cachedByFolder[folderKey].map((mail) => mail.id),
        );
        const uniquePage = page.filter((mail) => !existingIds.has(mail.id));
        state.cachedByFolder[folderKey].push(...uniquePage);
      }

      if (page.length > 0 || reset) {
        saveCachedEmails(state, storage);
      }

      pagination.nextOffset += page.length;

      if (page.length < batchSize) {
        pagination.hasMoreEmails = false;
      }
    } catch (error) {
      const message = error?.message || error?.msg || String(error);
      if (getEmails(state).length > 0) {
        ui.showMessage("Showing cached emails while reconnecting.");
      } else {
        ui.showMessage(`Failed to load emails: ${message}`, true);
      }
    } finally {
      state.isLoadingEmails = false;
      renderer.renderList();
    }
  }

  async function runInitialSync() {
    async function syncFolderWithRetries(
      folder,
      {
        maxAttempts = INITIAL_EMPTY_RETRY_MAX,
        showProgress = true,
        progressBase = 0,
        progressSpan = 100,
        progressLabel = folder,
      } = {},
    ) {
      let page = [];

      for (let attempt = 1; attempt <= maxAttempts; attempt += 1) {
        if (!state.allowFetch) {
          return { page, aborted: true };
        }

        if (showProgress) {
          const ratio = attempt / maxAttempts;
          const progress = Math.round(progressBase + ratio * progressSpan);
          ui.setSyncUiState(true, `Syncing ${progressLabel}...`, progress);
        }

        try {
          const fetched = await invoke("fetch_emails_handler", {
            minRange: 0,
            maxRange: INITIAL_BATCH_SIZE,
            mailbox: folder,
            provider: getProviderLiteral(storage),
          });
          page = Array.isArray(fetched) ? fetched : [];
        } catch (_error) {
          page = [];
        }

        if (page.length > 0) {
          break;
        }

        await delay(INITIAL_EMPTY_RETRY_DELAY_MS);
      }

      state.cachedByFolder[folder] = page;
      const folderPagination = getFolderPaginationState(state, folder);
      folderPagination.nextOffset = page.length;
      folderPagination.hasMoreEmails = page.length >= INITIAL_BATCH_SIZE;
      folderPagination.lastFetchSignature = null;
      folderPagination.lastFetchAt = 0;

      saveCachedEmails(state, storage);
      renderer.renderList();
      renderer.updateCounts();

      return { page, aborted: false };
    }

    ui.setSyncUiState(true, "Syncing inbox...", 2);

    const inboxSync = await syncFolderWithRetries("INBOX", {
      showProgress: true,
      progressBase: 0,
      progressSpan: 95,
      progressLabel: "INBOX",
    });

    if (inboxSync.aborted) {
      return;
    }

    if (inboxSync.page.length > 0) {
      state.isInitialSyncComplete = true;
      clearInitialSyncPoll();
      ui.setSyncUiState(false);

      // Sent sync should not block first render when inbox is already ready.
      void syncFolderWithRetries("Sent", {
        showProgress: false,
      }).catch(() => {
        // Ignore background sync failures; manual refresh can retry.
      });

      return;
    }

    const sentSync = await syncFolderWithRetries("Sent", {
      maxAttempts: Math.max(1, Math.floor(INITIAL_EMPTY_RETRY_MAX / 3)),
      showProgress: true,
      progressBase: 95,
      progressSpan: 5,
      progressLabel: "Sent",
    });

    if (sentSync.aborted) {
      return;
    }

    const hasAnySyncedData =
      state.cachedByFolder.INBOX.length > 0 ||
      state.cachedByFolder.Sent.length > 0;

    state.isInitialSyncComplete = hasAnySyncedData;

    if (hasAnySyncedData) {
      clearInitialSyncPoll();
      ui.setSyncUiState(false);
      return;
    }

    ui.setSyncUiState(true, "Syncing mailbox... waiting for first data", 99);

    clearInitialSyncPoll();
    state.initialSyncPollTimer = setTimeout(async () => {
      state.initialSyncPollTimer = null;

      if (!state.allowFetch || state.isInitialSyncComplete) {
        return;
      }

      await runInitialSync();
    }, INITIAL_EMPTY_RETRY_DELAY_MS);
  }

  async function refreshActiveMailbox() {
    if (!state.allowFetch || !state.isAppReady || state.isLoadingEmails) {
      return 0;
    }

    const folderKey = getActiveBackendFolderKey(state);
    const mailbox = getActiveMailboxLiteral(state);
    const provider = getProviderLiteral(storage);
    const pagination = getFolderPaginationState(state, folderKey);

    state.isLoadingEmails = true;
    renderer.renderList();

    try {
      const fetched = await invoke("refresh_emails_handler", {
        mailbox,
        provider,
      });

      const latest = Array.isArray(fetched) ? fetched : [];

      if (latest.length === 0) {
        return 0;
      }

      const existingIds = new Set(
        state.cachedByFolder[folderKey].map((mail) => mail.id),
      );
      const uniqueLatest = latest.filter((mail) => !existingIds.has(mail.id));

      if (uniqueLatest.length === 0) {
        return 0;
      }

      state.cachedByFolder[folderKey] = [
        ...uniqueLatest,
        ...state.cachedByFolder[folderKey],
      ];

      pagination.nextOffset += uniqueLatest.length;
      pagination.lastFetchSignature = null;
      pagination.lastFetchAt = 0;

      saveCachedEmails(state, storage);

      return uniqueLatest.length;
    } finally {
      state.isLoadingEmails = false;
      renderer.renderList();
    }
  }

  async function checkInitStatus() {
    try {
      console.log("Checking initialization status");
      const status = await invoke("check_init_status");

      console.log("Initialization status:", status);
      const isSignedIn = status === "signed_in";

      if (status === "setup") {
        storage.removeItem(EMAIL_CACHE_KEY);
        window.location.replace("/setup.html");
        return false;
      }

      if (status === "login") {
        storage.removeItem(EMAIL_CACHE_KEY);
        window.location.replace("/login.html");
        return false;
      }

      return isSignedIn;
    } catch (_error) {
      ui.showMessage("Failed to check initialization status.", true);
      return false;
    }
  }

  return {
    checkInitStatus,
    clearInitialEmptyRetry,
    clearInitialSyncPoll,
    loadMoreEmails,
    refreshActiveMailbox,
    runInitialSync,
    startBootstrapRefresh,
    stopBootstrapRefresh,
  };
}
