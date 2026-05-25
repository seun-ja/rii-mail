const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const INITIAL_BATCH_SIZE = 300;
const NEXT_BATCH_SIZE = 50;
const INITIAL_EMPTY_RETRY_DELAY_MS = 3000;
const INITIAL_EMPTY_RETRY_MAX = 30;
const AUTO_BOOTSTRAP_REFRESH_INTERVAL_MS = 3000;
const AUTO_BOOTSTRAP_REFRESH_MAX = 20;
const EMAIL_CACHE_KEY = "pemail.cachedEmails.v1";
const MAX_CACHED_EMAILS = 1200;

window.addEventListener("DOMContentLoaded", () => {
  const folderButtons = document.querySelectorAll(".folder-btn");
  const mailBodyEl = document.querySelector(".mail-body");
  const mailListPanelEl = document.querySelector(".mail-list-panel");
  const mailListEl = document.querySelector("#mail-list");
  const mailReaderEl = document.querySelector("#mail-reader");
  const sidebarToggleBtnEl = document.querySelector("#sidebar-toggle-btn");
  const readerResizerEl = document.querySelector("#reader-resizer");
  const searchInputEl = document.querySelector("#mail-search");
  const composeBtnEl = document.querySelector("#compose-btn");
  const refreshBtnEl = document.querySelector("#refresh-btn");
  const spamCheckBtnEl = document.querySelector("#spam-check-btn");
  const markReadBtnEl = document.querySelector("#mark-read-btn");
  const archiveBtnEl = document.querySelector("#archive-btn");
  const resultMsgEl = document.querySelector("#result-msg");
  const syncOverlayEl = document.querySelector("#sync-overlay");
  const syncMessageEl = document.querySelector("#sync-message");
  const syncProgressBarEl = document.querySelector("#sync-progress-bar");

  if (listen) {
    listen("app://logged-out", () => {
      state.allowFetch = false;
      state.isAppReady = false;
      stopBootstrapRefresh();
      clearInitialEmptyRetry();
      clearInitialSyncPoll();
      window.localStorage.removeItem(EMAIL_CACHE_KEY);
      state.cachedByFolder.INBOX = [];
      state.cachedByFolder.Sent = [];
      state.cachedByFolder.Trash = [];
      window.location.replace("/setup.html");
    }).catch(() => {
      // Ignore listener setup failures in non-Tauri contexts.
    });
  }

  const state = {
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

  let toastTimer = null;

  function setSyncUiState(locked, message = "Syncing...", percent = 0) {
    if (!syncOverlayEl || !syncMessageEl || !syncProgressBarEl) {
      return;
    }

    syncOverlayEl.classList.toggle("hidden", !locked);
    syncMessageEl.textContent = message;
    syncProgressBarEl.style.width = `${Math.max(0, Math.min(100, percent))}%`;
    syncOverlayEl
      .querySelector("[role='progressbar']")
      ?.setAttribute(
        "aria-valuenow",
        String(Math.round(Math.max(0, Math.min(100, percent)))),
      );

    document
      .querySelector(".mail-app")
      ?.classList.toggle("sync-locked", locked);
  }

  function delay(ms) {
    return new Promise((resolve) => {
      window.setTimeout(resolve, ms);
    });
  }

  function clearInitialSyncPoll() {
    if (state.initialSyncPollTimer) {
      clearTimeout(state.initialSyncPollTimer);
      state.initialSyncPollTimer = null;
    }
  }

  function getEmails() {
    return [
      ...state.cachedByFolder.INBOX,
      ...state.cachedByFolder.Sent,
      ...state.cachedByFolder.Trash,
    ];
  }

  function getActiveBackendFolderKey() {
    if (state.activeFolder === "Sent") {
      return "Sent";
    }

    if (state.activeFolder === "Trash") {
      return "Trash";
    }

    return "INBOX";
  }

  function getActiveMailboxLiteral() {
    return getActiveBackendFolderKey();
  }

  function getProviderLiteral() {
    const savedImapServer = (
      window.localStorage.getItem("pemail.imapServer") || ""
    ).toLowerCase();

    if (savedImapServer.includes("yahoo")) {
      window.localStorage.setItem("pemail.provider", "yahoo");
      return "yahoo";
    }

    if (savedImapServer.includes("gmail")) {
      window.localStorage.setItem("pemail.provider", "gmail");
      return "gmail";
    }

    const savedProvider = window.localStorage.getItem("pemail.provider");
    if (savedProvider === "gmail" || savedProvider === "yahoo") {
      return savedProvider;
    }

    return "yahoo";
  }

  function loadCachedEmails() {
    try {
      const raw = window.localStorage.getItem(EMAIL_CACHE_KEY);
      if (!raw) {
        return [];
      }

      const parsed = JSON.parse(raw);
      if (!Array.isArray(parsed)) {
        return [];
      }

      return parsed;
    } catch (_error) {
      return [];
    }
  }

  function saveCachedEmails() {
    try {
      const payload = getEmails().slice(0, MAX_CACHED_EMAILS);
      window.localStorage.setItem(EMAIL_CACHE_KEY, JSON.stringify(payload));
    } catch (_error) {
      // Ignore storage failures (quota/privacy mode).
    }
  }

  function hydrateFolderCachesFromStorage() {
    const cached = loadCachedEmails();

    state.cachedByFolder.INBOX = [];
    state.cachedByFolder.Sent = [];
    state.cachedByFolder.Trash = [];

    cached.forEach((mail) => {
      const folder = String(mail?.folder || "").toLowerCase();

      if (
        folder === "sent" ||
        (typeof mail?.id === "string" && mail.id.startsWith("db-Sent-"))
      ) {
        state.cachedByFolder.Sent.push({ ...mail, folder: "Sent" });
      } else if (folder === "trash" || folder === "archive") {
        state.cachedByFolder.Trash.push({ ...mail, folder: "Trash" });
      } else {
        state.cachedByFolder.INBOX.push({ ...mail, folder: "INBOX" });
      }
    });
  }

  function showMessage(text, isError = false) {
    resultMsgEl.textContent = text;
    resultMsgEl.classList.add("show");
    resultMsgEl.classList.toggle("error", isError);

    if (toastTimer) {
      clearTimeout(toastTimer);
    }

    toastTimer = setTimeout(() => {
      resultMsgEl.classList.remove("show");
    }, 2800);
  }

  function syncLayoutState() {
    if (!mailBodyEl) {
      return;
    }

    mailBodyEl.classList.toggle("sidebar-collapsed", state.isSidebarCollapsed);

    if (sidebarToggleBtnEl) {
      sidebarToggleBtnEl.setAttribute(
        "aria-expanded",
        String(!state.isSidebarCollapsed),
      );
      sidebarToggleBtnEl.setAttribute(
        "aria-label",
        state.isSidebarCollapsed
          ? "Expand mailbox section"
          : "Collapse mailbox section",
      );
      sidebarToggleBtnEl.classList.toggle(
        "is-collapsed",
        state.isSidebarCollapsed,
      );
    }

    if (window.innerWidth > 980 && mailListPanelEl) {
      setReaderListWidth(mailListPanelEl.getBoundingClientRect().width);
    }
  }

  function setReaderListWidth(nextWidthPx) {
    if (!mailBodyEl) {
      return;
    }

    const bodyRect = mailBodyEl.getBoundingClientRect();
    const sidebarWidth = state.isSidebarCollapsed ? 74 : 220;
    const resizerWidth = 10;
    const minListWidth = 260;
    const minReaderWidth = 360;
    const maxListWidth = Math.max(
      minListWidth,
      bodyRect.width - sidebarWidth - resizerWidth - minReaderWidth,
    );
    const clamped = Math.max(minListWidth, Math.min(nextWidthPx, maxListWidth));

    mailBodyEl.style.setProperty(
      "--list-panel-width",
      `${Math.round(clamped)}px`,
    );
  }

  function onReaderResizeMove(event) {
    if (!state.isResizingReader || !mailBodyEl) {
      return;
    }

    const bodyRect = mailBodyEl.getBoundingClientRect();
    const sidebarWidth = state.isSidebarCollapsed ? 74 : 220;
    const relativeX = event.clientX - bodyRect.left - sidebarWidth;

    setReaderListWidth(relativeX);
  }

  function onReaderResizeEnd() {
    if (!state.isResizingReader) {
      return;
    }

    state.isResizingReader = false;
    document.body.style.cursor = "";
    if (readerResizerEl) {
      readerResizerEl.classList.remove("is-dragging");
    }
    window.removeEventListener("pointermove", onReaderResizeMove);
    window.removeEventListener("pointerup", onReaderResizeEnd);
    window.removeEventListener("pointercancel", onReaderResizeEnd);
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

  function startBootstrapRefresh() {
    stopBootstrapRefresh();
    state.bootstrapRefreshAttempts = 0;

    state.bootstrapRefreshTimer = setInterval(() => {
      if (
        getEmails().length > 0 ||
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

  function getVisibleEmails() {
    const query = state.query.trim().toLowerCase();
    const inFolder = getEmails().filter((mail) => {
      if (state.activeFolder === "starred") {
        return mail.starred;
      }

      return (
        String(mail.folder || "").toLowerCase() ===
        state.activeFolder.toLowerCase()
      );
    });

    if (!query) {
      return inFolder;
    }

    return inFolder.filter((mail) => {
      const haystack =
        `${mail.senderName} ${mail.subject} ${mail.preview}`.toLowerCase();
      return haystack.includes(query);
    });
  }

  function updateCounts() {
    const counts = {
      inbox: getEmails().filter(
        (mail) => String(mail.folder || "").toLowerCase() === "inbox",
      ).length,
      starred: getEmails().filter((mail) => mail.starred).length,
      sent: getEmails().filter(
        (mail) => String(mail.folder || "").toLowerCase() === "sent",
      ).length,
      archive: getEmails().filter((mail) => {
        const folder = String(mail.folder || "").toLowerCase();
        return folder === "trash" || folder === "archive";
      }).length,
    };

    document.querySelector("#count-inbox").textContent = String(counts.inbox);
    document.querySelector("#count-starred").textContent = String(
      counts.starred,
    );
    document.querySelector("#count-sent").textContent = String(counts.sent);
    document.querySelector("#count-archive").textContent = String(
      counts.archive,
    );
  }

  function renderReader() {
    const selected = getEmails().find((mail) => mail.id === state.selectedId);

    if (!selected) {
      mailReaderEl.className = "mail-reader empty-state";
      mailReaderEl.innerHTML = `
        <div>
          <h2>No email selected</h2>
          <p>Select an email from the list to preview details.</p>
        </div>
      `;
      return;
    }

    selected.read = true;

    const hasHtmlBody =
      typeof selected.htmlBody === "string" &&
      selected.htmlBody.trim().length > 0;
    const textBody =
      (typeof selected.textBody === "string" &&
      selected.textBody.trim().length > 0
        ? selected.textBody
        : selected.body) || "No message body";

    mailReaderEl.className = "mail-reader";
    mailReaderEl.innerHTML = `
      <div class="reader-head">
        <h2 class="reader-subject">${selected.subject}</h2>
        <div class="reader-meta">
          <span><strong>From:</strong> ${selected.senderName} &lt;${selected.emailFrom}&gt;</span>
          <span><strong>Folder:</strong> ${selected.folder}</span>
          <span><strong>Received:</strong> ${selected.time}</span>
        </div>
      </div>
      ${
        hasHtmlBody
          ? '<iframe class="reader-html-frame" sandbox="allow-popups allow-popups-to-escape-sandbox" referrerpolicy="no-referrer"></iframe>'
          : `<div class="reader-body reader-body-text">${textBody}</div>`
      }
    `;

    if (hasHtmlBody) {
      const frame = mailReaderEl.querySelector(".reader-html-frame");
      if (frame) {
        frame.srcdoc = selected.htmlBody;
      }
    }
  }

  function renderList() {
    const visibleEmails = getVisibleEmails();

    if (visibleEmails.length === 0) {
      if (state.isLoadingEmails) {
        const syncLabel =
          state.syncProgressPercent > 0
            ? `Syncing... ${state.syncProgressPercent}%`
            : "Loading emails...";
        mailListEl.innerHTML = `
          <li class="mail-item">
            <div class="mail-item-content">
              <div class="mail-item-subject">${syncLabel}</div>
              <div class="mail-item-preview">Please wait while we fetch your mailbox.</div>
            </div>
          </li>
        `;
      } else {
        mailListEl.innerHTML = `
          <li class="mail-item">
            <div class="mail-item-content">
              <div class="mail-item-subject">No messages found</div>
              <div class="mail-item-preview">Try a different folder or search term.</div>
            </div>
          </li>
        `;
      }

      state.selectedId = null;
      updateCounts();
      renderReader();
      return;
    }

    if (!visibleEmails.some((mail) => mail.id === state.selectedId)) {
      state.selectedId = visibleEmails[0].id;
    }

    mailListEl.innerHTML = visibleEmails
      .map((mail) => {
        const initials = mail.senderName.slice(0, 1).toUpperCase();
        return `
          <li class="mail-item ${mail.read ? "" : "unread"} ${state.selectedId === mail.id ? "active" : ""}" data-id="${mail.id}">
            <div class="mail-item-avatar">${initials}</div>
            <div class="mail-item-content">
              <div class="mail-item-row">
                <span class="mail-item-sender">${mail.senderName}</span>
                <span class="mail-item-time">${mail.time}</span>
              </div>
              <div class="mail-item-subject">${mail.subject}</div>
              <div class="mail-item-preview">${mail.preview}</div>
            </div>
          </li>
        `;
      })
      .join("");

    renderReader();
    updateCounts();
  }

  async function loadMoreEmails(
    batchSize,
    { reset = false, preserveEmptyRetryState = false } = {},
  ) {
    if (!state.allowFetch) {
      return;
    }

    if (!state.isAppReady) {
      return;
    }

    if (state.isLoadingEmails) {
      return;
    }

    if (!reset && !state.hasMoreEmails) {
      return;
    }

    const folderKey = getActiveBackendFolderKey();

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

    // Guard against back-to-back duplicate retries for the same page.
    if (
      state.lastFetchSignature === fetchSignature &&
      now - state.lastFetchAt < 1200
    ) {
      return;
    }

    state.lastFetchSignature = fetchSignature;
    state.lastFetchAt = now;

    state.isLoadingEmails = true;
    renderList();

    try {
      const mailbox = getActiveMailboxLiteral();
      const provider = getProviderLiteral();

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
        saveCachedEmails();
      }

      state.nextOffset += page.length;

      if (page.length < batchSize) {
        state.hasMoreEmails = false;
      }
    } catch (error) {
      const message = error?.message || error?.msg || String(error);
      if (getEmails().length > 0) {
        showMessage("Showing cached emails while reconnecting.");
      } else {
        showMessage(`Failed to load emails: ${message}`, true);
      }
    } finally {
      state.isLoadingEmails = false;
      renderList();
    }
  }

  async function runInitialSync() {
    const folders = ["INBOX", "Sent"];
    setSyncUiState(true, "Syncing mailbox...", 2);

    for (let i = 0; i < folders.length; i += 1) {
      const folder = folders[i];
      let page = [];

      for (let attempt = 1; attempt <= INITIAL_EMPTY_RETRY_MAX; attempt += 1) {
        if (!state.allowFetch) {
          return;
        }

        const progress = Math.round(
          ((i + attempt / INITIAL_EMPTY_RETRY_MAX) / folders.length) * 100,
        );
        setSyncUiState(true, `Syncing ${folder}...`, progress);

        try {
          const fetched = await invoke("fetch_emails_handler", {
            minRange: 0,
            maxRange: INITIAL_BATCH_SIZE,
            mailbox: folder,
            provider: getProviderLiteral(),
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
      saveCachedEmails();
      renderList();
      updateCounts();
    }

    const hasAnySyncedData =
      state.cachedByFolder.INBOX.length > 0 ||
      state.cachedByFolder.Sent.length > 0;

    state.isInitialSyncComplete = hasAnySyncedData;

    if (hasAnySyncedData) {
      clearInitialSyncPoll();
      setSyncUiState(false);
      return;
    }

    setSyncUiState(true, "Syncing mailbox... waiting for first data", 99);

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
        window.localStorage.removeItem(EMAIL_CACHE_KEY);
        window.location.replace("/setup.html");
        return false;
      }

      if (status === "login") {
        window.localStorage.removeItem(EMAIL_CACHE_KEY);
        window.location.replace("/login.html");
        return false;
      }

      return isSignedIn;
    } catch (_err) {
      showMessage("Failed to check initialization status.", true);
      return false;
    }
  }

  folderButtons.forEach((button) => {
    button.addEventListener("click", () => {
      clearInitialEmptyRetry();
      state.initialEmptyRetries = 0;
      state.syncProgressPercent = 0;
      state.activeFolder = button.dataset.folder;
      folderButtons.forEach((otherButton) =>
        otherButton.classList.remove("active"),
      );
      button.classList.add("active");
      renderList();

      if (state.activeFolder === "INBOX" || state.activeFolder === "Sent") {
        const folderKey = getActiveBackendFolderKey();
        if (state.cachedByFolder[folderKey].length === 0) {
          loadMoreEmails(INITIAL_BATCH_SIZE, { reset: true });
        }
      }
    });
  });

  if (sidebarToggleBtnEl) {
    sidebarToggleBtnEl.addEventListener("click", () => {
      state.isSidebarCollapsed = !state.isSidebarCollapsed;
      syncLayoutState();
    });
  }

  if (readerResizerEl) {
    readerResizerEl.addEventListener("pointerdown", (event) => {
      if (window.innerWidth <= 980) {
        return;
      }

      event.preventDefault();
      state.isResizingReader = true;
      document.body.style.cursor = "col-resize";
      readerResizerEl.classList.add("is-dragging");

      window.addEventListener("pointermove", onReaderResizeMove);
      window.addEventListener("pointerup", onReaderResizeEnd);
      window.addEventListener("pointercancel", onReaderResizeEnd);
    });
  }

  window.addEventListener("resize", () => {
    if (window.innerWidth <= 980) {
      return;
    }

    if (mailListPanelEl) {
      setReaderListWidth(mailListPanelEl.getBoundingClientRect().width);
    }
  });

  mailListEl.addEventListener("click", (event) => {
    const target = event.target.closest(".mail-item");

    if (!target?.dataset?.id) {
      return;
    }

    state.selectedId = target.dataset.id;
    renderList();
  });

  searchInputEl.addEventListener("input", () => {
    state.query = searchInputEl.value;
    renderList();
  });

  mailListEl.addEventListener("scroll", () => {
    if (state.isLoadingEmails || !state.hasMoreEmails) {
      return;
    }

    const threshold = 40;
    const reachedBottom =
      mailListEl.scrollTop + mailListEl.clientHeight >=
      mailListEl.scrollHeight - threshold;

    if (reachedBottom) {
      loadMoreEmails(NEXT_BATCH_SIZE);
    }
  });

  composeBtnEl.addEventListener("click", () => {
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
    folderButtons.forEach((otherButton) => {
      otherButton.classList.toggle(
        "active",
        otherButton.dataset.folder === "Sent",
      );
    });
    state.selectedId = draftId;
    renderList();
    saveCachedEmails();
    showMessage("Draft created in Sent.");
  });

  refreshBtnEl.addEventListener("click", () => {
    loadMoreEmails(INITIAL_BATCH_SIZE, { reset: true });
    showMessage("Mailbox refreshed.");
  });

  markReadBtnEl.addEventListener("click", () => {
    const selected = getEmails().find((mail) => mail.id === state.selectedId);

    if (!selected) {
      showMessage("Select an email first.", true);
      return;
    }

    selected.read = true;
    renderList();
    saveCachedEmails();
    showMessage("Marked as read.");
  });

  archiveBtnEl.addEventListener("click", () => {
    const selected = getEmails().find((mail) => mail.id === state.selectedId);

    if (!selected) {
      showMessage("Select an email first.", true);
      return;
    }

    selected.folder = "Trash";
    state.selectedId = null;
    renderList();
    saveCachedEmails();
    showMessage("Email moved to archive.");
  });

  spamCheckBtnEl.addEventListener("click", async () => {
    const selected = getEmails().find((mail) => mail.id === state.selectedId);

    if (!selected) {
      showMessage("Select an email first.", true);
      return;
    }

    if (!invoke) {
      showMessage("No backend connected. Dummy spam check only.", true);
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
      showMessage(`Spam rating: ${label} (score: ${score})`);
    } catch (error) {
      showMessage(`Failed to check spam rating: ${error}`, true);
    }
  });

  hydrateFolderCachesFromStorage();
  syncLayoutState();
  if (getEmails().length > 0) {
    renderList();
  }

  (async () => {
    setSyncUiState(true, "Syncing mailbox...", 0);

    const isReady = await checkInitStatus();
    if (!isReady) {
      return;
    }

    state.isAppReady = true;
    await runInitialSync();

    if (!state.isInitialSyncComplete) {
      return;
    }

    setSyncUiState(false);
  })();
});
