export function createInitialState() {
  return {
    emails: [],
    cachedByFolder: {
      INBOX: [],
      Sent: [],
      Trash: [],
    },
    totalEmailsByFolder: {
      INBOX: null,
      Sent: null,
      Trash: null,
    },
    activeFolder: "INBOX",
    selectedId: null,
    query: "",
    isAppReady: false,
    isLoadingEmails: false,
    emailPopulationProgress: 0,
    isEmailPopulationComplete: false,
    paginationByFolder: {
      INBOX: {
        nextOffset: 0,
        hasMoreEmails: true,
        lastFetchSignature: null,
        lastFetchAt: 0,
      },
      Sent: {
        nextOffset: 0,
        hasMoreEmails: true,
        lastFetchSignature: null,
        lastFetchAt: 0,
      },
      Trash: {
        nextOffset: 0,
        hasMoreEmails: true,
        lastFetchSignature: null,
        lastFetchAt: 0,
      },
    },
    initialEmptyRetries: 0,
    initialEmptyRetryTimer: null,
    bootstrapRefreshAttempts: 0,
    bootstrapRefreshTimer: null,
    isSidebarCollapsed: false,
    isResizingReader: false,
    syncProgressPercent: 0,
    allowFetch: true,
    isInitialSyncComplete: false,
    initialSyncPollTimer: null,
  };
}
