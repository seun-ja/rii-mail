import {
  AUTO_BOOTSTRAP_REFRESH_INTERVAL_MS,
  AUTO_BOOTSTRAP_REFRESH_MAX,
  EMAIL_CACHE_KEY,
  INITIAL_BATCH_SIZE,
  INITIAL_EMPTY_RETRY_DELAY_MS,
  INITIAL_EMPTY_RETRY_MAX,
} from "./constants.js";
import { getEmails, saveCachedEmails } from "./cache.js";
import { delay, getActiveBackendFolderKey, getActiveMailboxLiteral } from "./helpers.js";
import { getProviderLiteral } from "./provider.js";

export function createSyncController(state, renderer, ui, invoke, storage = window.localStorage) {
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
      if (getEmails(state).length > 0 || state.bootstrapRefreshAttempts >= AUTO_BOOTSTRAP_REFRESH_MAX) {
        stopBootstrapRefresh();
        return;
      }

      if (!state.isLoadingEmails) {
        loadMoreEmails(INITIAL_BATCH_SIZE, { reset: true });
      }

      state.bootstrapRefreshAttempts += 1;
    }, AUTO_BOOTSTRAP_REFRESH_INTERVAL_MS);
  }

  async function loadMoreEmails(batchSize, { reset = false, preserveEmptyRetryState = false } = {}) {
    if (!state.allowFetch || !state.isAppReady || state.isLoadingEmails) {
      return;
    }

    if (!reset && !state.hasMoreEmails) {
      return;
    }

    const folderKey = getActiveBackendFolderKey(state);

    if (reset) {
      const hasExistingData = state.cachedByFolder[folderKey].length > 0;
      if (!hasExistingData) {
        state.selectedId = null;
      }
      state.nextOffset = 0;
      state.hasMoreEmails = true;

      if (!preserveEmptyRetryState) {
        state.initialEmptyRetries = 0;
        clearInitialEmptyRetry();
      }
    }

    const minRange = state.nextOffset;
    const maxRange = minRange + batchSize;
    const fetchSignature = `${state.activeFolder}:${minRange}:${maxRange}`;
    const now = Date.now();

    if (state.lastFetchSignature === fetchSignature && now - state.lastFetchAt < 1200) {
      return;
    }

    state.lastFetchSignature = fetchSignature;
    state.lastFetchAt = now;

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
        const existingIds = new Set(state.cachedByFolder[folderKey].map((mail) => mail.id));
        const uniquePage = page.filter((mail) => !existingIds.has(mail.id));
        state.cachedByFolder[folderKey].push(...uniquePage);
      }

      if (page.length > 0 || reset) {
        saveCachedEmails(state, storage);
      }

      state.nextOffset += page.length;

      if (page.length < batchSize) {
        state.hasMoreEmails = false;
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
    const folders = ["INBOX", "Sent"];
    ui.setSyncUiState(true, "Syncing mailbox...", 2);

    for (let i = 0; i < folders.length; i += 1) {
      const folder = folders[i];
      let page = [];

      for (let attempt = 1; attempt <= INITIAL_EMPTY_RETRY_MAX; attempt += 1) {
        if (!state.allowFetch) {
          return;
        }

        const progress = Math.round(((i + attempt / INITIAL_EMPTY_RETRY_MAX) / folders.length) * 100);
        ui.setSyncUiState(true, `Syncing ${folder}...`, progress);

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
      if (folder === getActiveBackendFolderKey(state)) {
        state.nextOffset = page.length;
      }

      saveCachedEmails(state, storage);
      renderer.renderList();
      renderer.updateCounts();
    }

    const hasAnySyncedData =
      state.cachedByFolder.INBOX.length > 0 || state.cachedByFolder.Sent.length > 0;

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

  async function checkInitStatus() {
    try {
      const status = await invoke("check_init_status");
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
    runInitialSync,
    startBootstrapRefresh,
    stopBootstrapRefresh,
  };
}
