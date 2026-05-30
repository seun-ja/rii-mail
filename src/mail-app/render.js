import { getEmails } from "./cache.js";

export function createRenderer(state, dom) {
  function getVisibleEmails() {
    const query = state.query.trim().toLowerCase();
    const inFolder = getEmails(state).filter((mail) => {
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
    const allEmails = getEmails(state);
    const counts = {
      inbox: allEmails.filter(
        (mail) => String(mail.folder || "").toLowerCase() === "inbox",
      ).length,
      starred: allEmails.filter((mail) => mail.starred).length,
      sent: allEmails.filter(
        (mail) => String(mail.folder || "").toLowerCase() === "sent",
      ).length,
      archive: allEmails.filter((mail) => {
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
    const selected = getEmails(state).find(
      (mail) => mail.id === state.selectedId,
    );

    if (!selected) {
      dom.mailReaderEl.className = "mail-reader empty-state";
      dom.mailReaderEl.innerHTML = `
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

    dom.mailReaderEl.className = "mail-reader";
    dom.mailReaderEl.innerHTML = `
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
      const frame = dom.mailReaderEl.querySelector(".reader-html-frame");
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
        dom.mailListEl.innerHTML = `
          <li class="mail-item">
            <div class="mail-item-content">
              <div class="mail-item-subject">${syncLabel}</div>
              <div class="mail-item-preview">Please wait while we fetch your mailbox.</div>
            </div>
          </li>
        `;
      } else {
        dom.mailListEl.innerHTML = `
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

    dom.mailListEl.innerHTML = visibleEmails
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

  return {
    getVisibleEmails,
    renderReader,
    renderList,
    updateCounts,
  };
}
