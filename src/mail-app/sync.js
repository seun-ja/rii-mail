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
import { createMailboxCountClient } from "./mailbox-counts.js";

export function createSyncController(
  state,
  renderer,
  ui,
  invoke,
  storage = window.localStorage,
) {
  let syncGeneration = 0;
  const { fetchMailboxEmailCount } = createMailboxCountClient({ storage });

  function normalizeFetchPayload(fetched) {
    if (fetched && typeof fetched === "object" && !Array.isArray(fetched)) {
      return {
        emails: Array.isArray(fetched.emails) ? fetched.emails : [],
      };
    }

    return {
      emails: Array.isArray(fetched) ? fetched : [],
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
    { reset = false, preserveEmptyRetryState = false, retryWhenBusy = false } = {},
  ) {
    if (!state.allowFetch || !state.isAppReady) {
      return;
    }

    if (state.isLoadingEmails) {
      if (retryWhenBusy && !state.initialSyncPollTimer) {
        state.initialSyncPollTimer = setTimeout(() => {
          state.initialSyncPollTimer = null;
          void loadMoreEmails(batchSize, {
            reset,
            preserveEmptyRetryState,
            retryWhenBusy: true,
          });
        }, 100);
      }
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

      // The email page is required to render the mailbox. The count is only
      // metadata, so an unavailable count query must not discard a valid page.
      const fetched = await invoke("fetch_emails", {
        minRange,
        maxRange,
        mailbox,
        provider,
      });
      const totalEmails = await fetchMailboxEmailCount(mailbox, provider).catch(
        () => null,
      );

      const payload = normalizeFetchPayload(fetched);
      const page = payload.emails;

      if (typeof totalEmails === "number") {
        state.totalEmailsByFolder[folderKey] = totalEmails;
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

  async function refreshInboxOnStartup() {
    if (state.activeFolder !== "INBOX") {
      return;
    }

    try {
      await refreshActiveMailbox({ background: true });
    } catch (error) {
      console.warn("Unable to refresh inbox during startup", error);
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
          // A mailbox count failure must not make an available first page look
          // empty while the initial import is still running.
          const fetched = await invoke("fetch_emails", {
            minRange: 0,
            maxRange: INITIAL_BATCH_SIZE,
            mailbox: folder,
            provider,
          });
          const totalEmails = await fetchMailboxEmailCount(folder, provider).catch(
            () => null,
          );
          const payload = normalizeFetchPayload(fetched);
          page = payload.emails;

          if (typeof totalEmails === "number") {
            state.totalEmailsByFolder[folder] = totalEmails;
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

            const sentTotalEmails = await fetchMailboxEmailCount(
              "Sent",
              provider,
            );
            if (typeof sentTotalEmails === "number") {
              state.totalEmailsByFolder.Sent = sentTotalEmails;
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

    // `refresh_emails_handler` is serviced by the same serial IMAP worker as the
    // full initial import. Awaiting it here can leave startup locked until that
    // import finishes, even though the first database page is already usable.
    state.isInitialSyncComplete = true;
    clearInitialSyncPoll();
    ui.setSyncUiState(false);

    if (inboxSync.page.length > 0) {
      // A local page is already usable, so reconcile it without blocking UI.
      void refreshInboxOnStartup();

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
    // Keep warming Sent in the background even when inbox data arrives late.
    void syncFolderWithRetries("Sent", {
      showProgress: false,
      maxAttempts: Math.max(1, Math.floor(INITIAL_EMPTY_RETRY_MAX / 3)),
    }).catch(() => {
      // Ignore background sync failures; manual fetch still works on folder switch.
    });

    startBootstrapRefresh();
  }

  async function refreshActiveMailbox({ background = false } = {}) {
    if (!state.allowFetch || !state.isAppReady || state.isLoadingEmails) {
      return { newCount: 0, totalEmails: null };
    }

    const folderKey = getActiveBackendFolderKey(state);
    const mailbox = getActiveMailboxLiteral(state);
    const provider = getProviderLiteral(storage);
    const pagination = getFolderPaginationState(state, folderKey);

    if (!background) {
      state.isLoadingEmails = true;
      renderer.renderList();
    }

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

      const reportedNewCount = Number.isInteger(payload.newEmailsCount)
        ? Math.max(0, payload.newEmailsCount)
        : 0;
      const pageSize = Math.max(
        INITIAL_BATCH_SIZE,
        state.cachedByFolder[folderKey].length + reportedNewCount,
      );
      const refreshed = await invoke("fetch_emails", {
        minRange: 0,
        maxRange: pageSize,
        mailbox,
        provider,
      });
      const refreshedPage = normalizeFetchPayload(refreshed).emails;
      const storedTotalEmails = await fetchMailboxEmailCount(mailbox, provider).catch(
        () => null,
      );
      const totalEmails = Number.isInteger(payload.totalEmails)
        ? payload.totalEmails
        : typeof storedTotalEmails === "number"
          ? storedTotalEmails
          : null;

      if (typeof totalEmails === "number") {
        state.totalEmailsByFolder[folderKey] = totalEmails;
      }

      state.cachedByFolder[folderKey] = refreshedPage;
      pagination.nextOffset = refreshedPage.length;
      pagination.hasMoreEmails =
        typeof totalEmails === "number"
          ? refreshedPage.length < totalEmails
          : refreshedPage.length >= pageSize;
      pagination.lastFetchSignature = null;
      pagination.lastFetchAt = 0;

      saveCachedEmails(state, storage);

      return { newCount: reportedNewCount, totalEmails };
    } finally {
      if (!background) {
        state.isLoadingEmails = false;
      }
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
