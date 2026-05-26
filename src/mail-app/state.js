export function createInitialState() {
  return {
    emails: [],
    cachedByFolder: {
      INBOX: [],
      Sent: [],
      Trash: [],
    },
    activeFolder: "INBOX",
    selectedId: null,
    query: "",
    isAppReady: false,
    nextOffset: 0,
    isLoadingEmails: false,
    hasMoreEmails: true,
    initialEmptyRetries: 0,
    initialEmptyRetryTimer: null,
    bootstrapRefreshAttempts: 0,
    bootstrapRefreshTimer: null,
    isSidebarCollapsed: false,
    isResizingReader: false,
    lastFetchSignature: null,
    lastFetchAt: 0,
    syncProgressPercent: 0,
    allowFetch: true,
    isInitialSyncComplete: false,
    initialSyncPollTimer: null,
  };
}
