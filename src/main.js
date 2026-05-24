const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const INITIAL_BATCH_SIZE = 300;
const NEXT_BATCH_SIZE = 50;
const INITIAL_EMPTY_RETRY_DELAY_MS = 1500;
const INITIAL_EMPTY_RETRY_MAX = 30;
const AUTO_BOOTSTRAP_REFRESH_INTERVAL_MS = 3000;
const AUTO_BOOTSTRAP_REFRESH_MAX = 20;
const EMAIL_CACHE_KEY = "pemail.cachedEmails.v1";
const MAX_CACHED_EMAILS = 1200;

window.addEventListener("DOMContentLoaded", () => {
  const folderButtons = document.querySelectorAll(".folder-btn");
  const mailListEl = document.querySelector("#mail-list");
  const mailReaderEl = document.querySelector("#mail-reader");
  const searchInputEl = document.querySelector("#mail-search");
  const composeBtnEl = document.querySelector("#compose-btn");
  const refreshBtnEl = document.querySelector("#refresh-btn");
  const spamCheckBtnEl = document.querySelector("#spam-check-btn");
  const markReadBtnEl = document.querySelector("#mark-read-btn");
  const archiveBtnEl = document.querySelector("#archive-btn");
  const resultMsgEl = document.querySelector("#result-msg");

  if (listen) {
    listen("app://logged-out", () => {
      window.location.replace("/setup.html");
    }).catch(() => {
      // Ignore listener setup failures in non-Tauri contexts.
    });
  }

  const state = {
    emails: [],
    activeFolder: "inbox",
    selectedId: null,
    query: "",
    isAppReady: false,
    nextOffset: 0,
    isLoadingEmails: false,
    hasMoreEmails: true,
    initialEmptyRetries: 0,
    bootstrapRefreshAttempts: 0,
    bootstrapRefreshTimer: null,
  };

  let toastTimer = null;

  function getEmails() {
    return state.emails;
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
      const payload = state.emails.slice(0, MAX_CACHED_EMAILS);
      window.localStorage.setItem(EMAIL_CACHE_KEY, JSON.stringify(payload));
    } catch (_error) {
      // Ignore storage failures (quota/privacy mode).
    }
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

  function stopBootstrapRefresh() {
    if (state.bootstrapRefreshTimer) {
      clearInterval(state.bootstrapRefreshTimer);
      state.bootstrapRefreshTimer = null;
    }
  }

  function startBootstrapRefresh() {
    stopBootstrapRefresh();
    state.bootstrapRefreshAttempts = 0;

    state.bootstrapRefreshTimer = setInterval(() => {
      if (state.emails.length > 0 || state.bootstrapRefreshAttempts >= AUTO_BOOTSTRAP_REFRESH_MAX) {
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

      return mail.folder === state.activeFolder;
    });

    if (!query) {
      return inFolder;
    }

    return inFolder.filter((mail) => {
      const haystack = `${mail.senderName} ${mail.subject} ${mail.preview}`.toLowerCase();
      return haystack.includes(query);
    });
  }

  function updateCounts() {
    const counts = {
      inbox: getEmails().filter((mail) => mail.folder === "inbox").length,
      starred: getEmails().filter((mail) => mail.starred).length,
      sent: getEmails().filter((mail) => mail.folder === "sent").length,
      archive: getEmails().filter((mail) => mail.folder === "archive").length,
    };

    document.querySelector("#count-inbox").textContent = String(counts.inbox);
    document.querySelector("#count-starred").textContent = String(counts.starred);
    document.querySelector("#count-sent").textContent = String(counts.sent);
    document.querySelector("#count-archive").textContent = String(counts.archive);
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

    const hasHtmlBody = typeof selected.htmlBody === "string" && selected.htmlBody.trim().length > 0;
    const textBody =
      (typeof selected.textBody === "string" && selected.textBody.trim().length > 0
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
        mailListEl.innerHTML = `
          <li class="mail-item">
            <div class="mail-item-content">
              <div class="mail-item-subject">Loading emails...</div>
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

  async function loadMoreEmails(batchSize, { reset = false } = {}) {
    if (!state.isAppReady) {
      return;
    }

    if (state.isLoadingEmails) {
      return;
    }

    if (!reset && !state.hasMoreEmails) {
      return;
    }

    if (reset) {
      const hasExistingData = state.emails.length > 0;
      if (!hasExistingData) {
        state.emails = [];
        state.selectedId = null;
      }
      state.nextOffset = 0;
      state.hasMoreEmails = true;
      state.initialEmptyRetries = 0;
    }

    const minRange = state.nextOffset;
    const maxRange = minRange + batchSize;

    state.isLoadingEmails = true;
    renderList();

    try {
      const fetched = await invoke("fetch_emails_handler", {
        minRange,
        maxRange,
      });

      const page = Array.isArray(fetched) ? fetched : [];

      if (page.length > 0) {
        stopBootstrapRefresh();
      }

      if (reset) {
        state.emails = page;
      } else {
        const existingIds = new Set(state.emails.map((mail) => mail.id));
        const uniquePage = page.filter((mail) => !existingIds.has(mail.id));
        state.emails.push(...uniquePage);
      }

      if (page.length > 0 || reset) {
        saveCachedEmails();
      }

      state.nextOffset += page.length;

      if (page.length === 0 && minRange === 0 && state.initialEmptyRetries < INITIAL_EMPTY_RETRY_MAX) {
        state.initialEmptyRetries += 1;
        state.hasMoreEmails = true;

        const attemptLabel = `${state.initialEmptyRetries}/${INITIAL_EMPTY_RETRY_MAX}`;
        showMessage(`Syncing emails... (${attemptLabel})`);

        setTimeout(() => {
          loadMoreEmails(INITIAL_BATCH_SIZE, { reset: true });
        }, INITIAL_EMPTY_RETRY_DELAY_MS);
      } else if (page.length < batchSize) {
        state.hasMoreEmails = false;
      }

      if (page.length === 0 && minRange === 0) {
        if (state.initialEmptyRetries >= INITIAL_EMPTY_RETRY_MAX && state.emails.length === 0) {
          showMessage("No emails found in local database.");
        }
      }
    } catch (error) {
      const message = error?.message || error?.msg || String(error);
      if (state.emails.length > 0) {
        showMessage("Showing cached emails while reconnecting.");
      } else {
        showMessage(`Failed to load emails: ${message}`, true);
      }
    } finally {
      state.isLoadingEmails = false;
      renderList();
    }
  }

  async function checkInitStatus() {
    try {
      const status = await invoke("check_init_status");
      const isSignedIn = status === "signed_in" || status === "signedin";

      if (status === "setup") {
        window.location.replace("/setup.html");
        return false;
      }

      if (status === "login") {
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
      state.activeFolder = button.dataset.folder;
      folderButtons.forEach((otherButton) => otherButton.classList.remove("active"));
      button.classList.add("active");
      renderList();
    });
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
      mailListEl.scrollTop + mailListEl.clientHeight >= mailListEl.scrollHeight - threshold;

    if (reachedBottom) {
      loadMoreEmails(NEXT_BATCH_SIZE);
    }
  });

  composeBtnEl.addEventListener("click", () => {
    const draftId = `draft-${Date.now()}`;
    state.emails.unshift({
      id: draftId,
      folder: "sent",
      senderName: "You",
      emailFrom: "you@company.com",
      subject: "Draft: New message",
      preview: "This is a placeholder draft. Wire this to your compose modal later.",
      body: "Draft created from the home UI. Replace this with your backend compose flow.",
      time: "Now",
      starred: false,
      read: true,
    });

    state.activeFolder = "sent";
    folderButtons.forEach((otherButton) => {
      otherButton.classList.toggle("active", otherButton.dataset.folder === "sent");
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

    selected.folder = "archive";
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

      const score = typeof rating?.score === "number" ? rating.score.toFixed(2) : "N/A";
      const label = rating?.label ?? "Unknown";
      showMessage(`Spam rating: ${label} (score: ${score})`);
    } catch (error) {
      showMessage(`Failed to check spam rating: ${error}`, true);
    }
  });

  state.emails = loadCachedEmails();
  if (state.emails.length > 0) {
    renderList();
  }

  (async () => {
    const isReady = await checkInitStatus();
    if (!isReady) {
      return;
    }

    state.isAppReady = true;
    loadMoreEmails(INITIAL_BATCH_SIZE, { reset: true });
    startBootstrapRefresh();
  })();
});
