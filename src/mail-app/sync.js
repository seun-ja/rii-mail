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
  let syncGeneration = 0;

  function normalizeFetchPayload(fetched, folder = null) {
    // TODO: #4 return dynamic figures from backend instead of hardcoding defaults here. Backend should always return totalEmails for correct pagination, but we need to handle cases where it doesn't (e.g. due to older backend versions or errors) - in those cases we can use these conservative defaults to avoid breaking pagination entirely.
    const normalizedFolder =
      typeof folder === "string" ? folder.toLowerCase() : "";
    const defaultTotalEmails =
      normalizedFolder === "sent" ? 86 : 6200;

    if (fetched && typeof fetched === "object" && !Array.isArray(fetched)) {
      return {
        emails: Array.isArray(fetched.emails) ? fetched.emails : [],
        totalEmails:
          typeof fetched.totalEmails === "number"
            ? fetched.totalEmails
            : defaultTotalEmails,
      };
    }

    return {
      emails: Array.isArray(fetched) ? fetched : [],
      totalEmails: defaultTotalEmails,
    };
  }

  function cancelSyncRetries() {
    syncGeneration += 1;
  }

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

  function haltAllSync() {
    state.allowFetch = false;
    stopBootstrapRefresh();
    clearInitialEmptyRetry();
    clearInitialSyncPoll();
    cancelSyncRetries();
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

      const payload = normalizeFetchPayload(fetched, folderKey);
      const page = payload.emails;

      if (typeof payload.totalEmails === "number") {
        state.totalEmailsByFolder[folderKey] = payload.totalEmails;
      }

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
      const runGeneration = syncGeneration;
      let page = [];
      let didSentRefreshFallback = false;
      const provider = getProviderLiteral(storage);

      for (let attempt = 1; attempt <= maxAttempts; attempt += 1) {
        if (!state.allowFetch || runGeneration !== syncGeneration) {
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
            provider,
          });
          const payload = normalizeFetchPayload(fetched, folder);
          page = payload.emails;

          if (typeof payload.totalEmails === "number") {
            state.totalEmailsByFolder[folder] = payload.totalEmails;
          }
        } catch (_error) {
          page = [];
        }

        if (
          folder === "Sent" &&
          page.length === 0 &&
          !didSentRefreshFallback &&
          state.allowFetch &&
          runGeneration === syncGeneration
        ) {
          didSentRefreshFallback = true;

          try {
            const refreshed = await invoke("refresh_emails_handler", {
              mailbox: "Sent",
              provider,
            });
            const latest =
              refreshed &&
              typeof refreshed === "object" &&
              !Array.isArray(refreshed)
                ? refreshed
                : null;

            page = Array.isArray(latest?.emails) ? latest.emails : [];

            if (typeof latest?.totalEmails === "number") {
              state.totalEmailsByFolder.Sent = latest.totalEmails;
            }
          } catch (_error) {
            // Ignore fallback refresh failures and continue regular retry flow.
          }
        }

        if (page.length > 0) {
          break;
        }

        await delay(INITIAL_EMPTY_RETRY_DELAY_MS);

        if (!state.allowFetch || runGeneration !== syncGeneration) {
          return { page, aborted: true };
        }
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
      maxAttempts: Math.min(3, INITIAL_EMPTY_RETRY_MAX),
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

    // Do not block app startup for long when backend bootstrap is still filling DB.
    // Keep retrying first-page inbox fetch in the background.
    state.isInitialSyncComplete = true;
    clearInitialSyncPoll();
    ui.setSyncUiState(false);

    // Keep warming Sent in the background even when inbox data arrives late.
    void syncFolderWithRetries("Sent", {
      showProgress: false,
      maxAttempts: Math.max(1, Math.floor(INITIAL_EMPTY_RETRY_MAX / 3)),
    }).catch(() => {
      // Ignore background sync failures; manual fetch still works on folder switch.
    });

    startBootstrapRefresh();
  }

  async function refreshActiveMailbox() {
    if (!state.allowFetch || !state.isAppReady || state.isLoadingEmails) {
      return { newCount: 0, totalEmails: null };
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

      const payload =
        fetched && typeof fetched === "object" && !Array.isArray(fetched)
          ? fetched
          : {
              emails: Array.isArray(fetched) ? fetched : [],
              newEmailsCount: 0,
              totalEmails: null,
            };

      const latest = Array.isArray(payload.emails) ? payload.emails : [];
      const totalEmails =
        typeof payload.totalEmails === "number" ? payload.totalEmails : null;

      if (typeof totalEmails === "number") {
        state.totalEmailsByFolder[folderKey] = totalEmails;
      }

      if (latest.length === 0) {
        return { newCount: 0, totalEmails };
      }

      const existingIds = new Set(
        state.cachedByFolder[folderKey].map((mail) => mail.id),
      );
      const uniqueLatest = latest.filter((mail) => !existingIds.has(mail.id));

      if (uniqueLatest.length === 0) {
        return { newCount: 0, totalEmails };
      }

      state.cachedByFolder[folderKey] = [
        ...uniqueLatest,
        ...state.cachedByFolder[folderKey],
      ];

      pagination.nextOffset += uniqueLatest.length;
      pagination.lastFetchSignature = null;
      pagination.lastFetchAt = 0;

      saveCachedEmails(state, storage);

      return { newCount: uniqueLatest.length, totalEmails };
    } finally {
      state.isLoadingEmails = false;
      renderer.renderList();
    }
  }

  return {
    cancelSyncRetries,
    clearInitialEmptyRetry,
    clearInitialSyncPoll,
    haltAllSync,
    loadMoreEmails,
    refreshActiveMailbox,
    runInitialSync,
    startBootstrapRefresh,
    stopBootstrapRefresh,
  };
}
