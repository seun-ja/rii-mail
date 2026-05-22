const invoke = window.__TAURI__?.core?.invoke;
const listen = window.__TAURI__?.event?.listen;

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

  const dummyEmails = [
    {
      id: "m01",
      folder: "inbox",
      senderName: "GitHub",
      emailFrom: "notifications@github.com",
      subject: "[Action required] Review requested on PR #142",
      preview: "Seun has requested your review on the latest changes to the desktop mail client.",
      body: "Hi Seun,\n\nA review was requested on PR #142 in seun-ja/email-desktop-client.\n\nHighlights:\n- Refined sidebar interactions\n- Added message loading skeleton\n- Improved spam scoring model call path\n\nPlease review when convenient.\n\nThanks,\nGitHub",
      time: "08:41",
      starred: true,
      read: false,
    },
    {
      id: "m02",
      folder: "inbox",
      senderName: "Design Team",
      emailFrom: "ui@pemail.dev",
      subject: "Thunderbird-inspired navigation concepts",
      preview: "Attached concept v3 with denser list spacing and clearer hierarchy.",
      body: "Morning,\n\nWe shipped a tighter navigation concept with better contrast and scanning for power users.\n\nIf approved, we can map this to your existing components this sprint.",
      time: "07:12",
      starred: false,
      read: false,
    },
    {
      id: "m03",
      folder: "inbox",
      senderName: "Cloud Billing",
      emailFrom: "billing@provider.io",
      subject: "Invoice available for April 2026",
      preview: "Your monthly invoice is ready. Total due: $72.90.",
      body: "Hello,\n\nYour April invoice is now ready.\n\nTotal: $72.90\nDue date: May 25, 2026\n\nYou can download the full receipt from your billing portal.",
      time: "Yesterday",
      starred: false,
      read: true,
    },
    {
      id: "m04",
      folder: "sent",
      senderName: "You",
      emailFrom: "you@company.com",
      subject: "Re: Backend schema for message sync",
      preview: "I added draft fields for conversation id and thread position.",
      body: "Team,\n\nI added placeholders for conversation threading and sync cursor values.\n\nWill share migration notes shortly.",
      time: "Mon",
      starred: false,
      read: true,
    },
    {
      id: "m05",
      folder: "archive",
      senderName: "Meeting Bot",
      emailFrom: "noreply@meetings.io",
      subject: "Transcript: Weekly product sync",
      preview: "Transcript and action items are now available.",
      body: "Your meeting transcript is ready.\n\nAction items:\n1. Finalize inbox interactions\n2. Connect list view to backend API\n3. Add keyboard shortcuts",
      time: "Apr 18",
      starred: false,
      read: true,
    },
  ];

  const state = {
    activeFolder: "inbox",
    selectedId: null,
    query: "",
  };

  let toastTimer = null;

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

  function getVisibleEmails() {
    const query = state.query.trim().toLowerCase();
    const inFolder = dummyEmails.filter((mail) => {
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
      inbox: dummyEmails.filter((mail) => mail.folder === "inbox").length,
      starred: dummyEmails.filter((mail) => mail.starred).length,
      sent: dummyEmails.filter((mail) => mail.folder === "sent").length,
      archive: dummyEmails.filter((mail) => mail.folder === "archive").length,
    };

    document.querySelector("#count-inbox").textContent = String(counts.inbox);
    document.querySelector("#count-starred").textContent = String(counts.starred);
    document.querySelector("#count-sent").textContent = String(counts.sent);
    document.querySelector("#count-archive").textContent = String(counts.archive);
  }

  function renderReader() {
    const selected = dummyEmails.find((mail) => mail.id === state.selectedId);

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
      <div class="reader-body">${selected.body}</div>
    `;
  }

  function renderList() {
    const visibleEmails = getVisibleEmails();

    if (visibleEmails.length === 0) {
      mailListEl.innerHTML = `
        <li class="mail-item">
          <div class="mail-item-content">
            <div class="mail-item-subject">No messages found</div>
            <div class="mail-item-preview">Try a different folder or search term.</div>
          </div>
        </li>
      `;
      state.selectedId = null;
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

  async function checkInitStatus() {
    if (!invoke) {
      showMessage("Tauri runtime not available. Showing local demo.", true);
      return true;
    }

    try {
      const status = await invoke("check_init_status");

      if (status === "setup") {
        window.location.replace("/setup.html");
        return false;
      }

      if (status === "login") {
        window.location.replace("/login.html");
        return false;
      }

      return status === "signed_in";
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

  composeBtnEl.addEventListener("click", () => {
    const draftId = `draft-${Date.now()}`;
    dummyEmails.unshift({
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
    showMessage("Draft created in Sent.");
  });

  refreshBtnEl.addEventListener("click", () => {
    const nowTime = new Date().toLocaleTimeString([], {
      hour: "2-digit",
      minute: "2-digit",
    });
    dummyEmails[0].time = nowTime;
    renderList();
    showMessage("Mailbox refreshed.");
  });

  markReadBtnEl.addEventListener("click", () => {
    const selected = dummyEmails.find((mail) => mail.id === state.selectedId);

    if (!selected) {
      showMessage("Select an email first.", true);
      return;
    }

    selected.read = true;
    renderList();
    showMessage("Marked as read.");
  });

  archiveBtnEl.addEventListener("click", () => {
    const selected = dummyEmails.find((mail) => mail.id === state.selectedId);

    if (!selected) {
      showMessage("Select an email first.", true);
      return;
    }

    selected.folder = "archive";
    state.selectedId = null;
    renderList();
    showMessage("Email moved to archive.");
  });

  spamCheckBtnEl.addEventListener("click", async () => {
    const selected = dummyEmails.find((mail) => mail.id === state.selectedId);

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

  checkInitStatus().then((isReady) => {
    if (!isReady) {
      return;
    }

    updateCounts();
    renderList();
  });
});
