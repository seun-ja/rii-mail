import { getEmails } from "./cache.js";
import {
  EMAIL_LINK_MESSAGE,
  escapeHtml,
  formatAddresses,
  getSafeExternalUrl,
  linkifyPlainText,
  prepareEmailHtml,
} from "./helpers.js";

export function createRenderer(state, dom, openEmailLink) {
  let removeFrameLinkListener = () => {};

  function dispose() {
    removeFrameLinkListener();
    removeFrameLinkListener = () => {};
  }

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
      inbox:
        typeof state.totalEmailsByFolder?.INBOX === "number"
          ? state.totalEmailsByFolder.INBOX
          : null,
      starred: allEmails.filter((mail) => mail.starred).length,
      sent:
        typeof state.totalEmailsByFolder?.Sent === "number"
          ? state.totalEmailsByFolder.Sent
          : null,
      archive: allEmails.filter((mail) => {
        const folder = String(mail.folder || "").toLowerCase();
        return folder === "trash" || folder === "archive";
      }).length,
    };

    document.querySelector("#count-inbox").textContent =
      typeof counts.inbox === "number" ? String(counts.inbox) : "";
    document.querySelector("#count-starred").textContent = String(
      counts.starred,
    );
    document.querySelector("#count-sent").textContent =
      typeof counts.sent === "number" ? String(counts.sent) : "";
    document.querySelector("#count-archive").textContent = String(
      counts.archive,
    );
  }

  function renderReader() {
    dispose();
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

    const subject = escapeHtml(selected.subject || "(No subject)");
    const senderName = escapeHtml(selected.senderName || "Unknown sender");
    const emailFrom = escapeHtml(selected.emailFrom);
    const ccRecipients = formatAddresses(selected.cc).map(escapeHtml);
    const receivedAt = escapeHtml(selected.time);
    const ccMarkup = (() => {
      if (ccRecipients.length === 0) {
        return "";
      }

      if (ccRecipients.length <= 2) {
        return `<div class="reader-meta-item reader-meta-cc">
          <span class="reader-meta-label">Cc</span>
          <span class="reader-meta-value">${ccRecipients.join(", ")}</span>
        </div>`;
      }

      const preview = ccRecipients.slice(0, 2).join(", ");
      const remaining = ccRecipients.length - 2;

      return `<details class="reader-meta-item reader-meta-cc reader-meta-cc-collapsible">
        <summary class="reader-meta-cc-summary">
          <span class="reader-meta-label">Cc</span>
          <span class="reader-meta-value">${preview}</span>
          <span class="reader-meta-cc-more">+${remaining} more</span>
          <span class="reader-meta-cc-less">Show less</span>
        </summary>
        <div class="reader-meta-cc-expanded">${ccRecipients.join(", ")}</div>
      </details>`;
    })();

    dom.mailReaderEl.className = hasHtmlBody
      ? "mail-reader reader-html-message"
      : "mail-reader";
    dom.mailReaderEl.innerHTML = `
      <header class="reader-head">
        <div class="reader-title-row">
          <h2 class="reader-subject">${subject}</h2>
          <div class="reader-actions" aria-label="Message actions">
            <button class="reader-action-btn" type="button" data-reader-action="reply" aria-label="Reply" title="Reply">
              <svg viewBox="0 0 24 24" aria-hidden="true"><path d="m9 7-5 5 5 5M4 12h9a5 5 0 0 1 5 5" /></svg>
            </button>
            <button class="reader-action-btn" type="button" data-reader-action="forward" aria-label="Forward" title="Forward">
              <svg viewBox="0 0 24 24" aria-hidden="true"><path d="m15 7 5 5-5 5M20 12h-9a5 5 0 0 0-5 5" /></svg>
            </button>
          </div>
        </div>
        <div class="reader-meta">
          <div class="reader-meta-item reader-meta-sender">
            <span class="reader-meta-label">From</span>
            <span class="reader-meta-value">${senderName}${emailFrom ? ` &lt;${emailFrom}&gt;` : ""}</span>
          </div>
          <div class="reader-meta-item reader-meta-time">
            <span class="reader-meta-value">${receivedAt}</span>
          </div>
          ${ccMarkup}
        </div>
      </header>
      ${
        hasHtmlBody
          ? `<iframe class="reader-html-frame" title="Email content: ${subject}" sandbox="allow-scripts" referrerpolicy="no-referrer"></iframe>`
          : `<div class="reader-body reader-body-text">${linkifyPlainText(textBody)}</div>`
      }
    `;

    if (hasHtmlBody) {
      const frame = dom.mailReaderEl.querySelector(".reader-html-frame");
      if (frame) {
        const hostWindow = dom.mailReaderEl.ownerDocument.defaultView;
        const onFrameMessage = (event) => {
          if (
            event.source !== frame.contentWindow ||
            event.data?.type !== EMAIL_LINK_MESSAGE ||
            typeof event.data.url !== "string"
          ) {
            return;
          }

          const safeUrl = getSafeExternalUrl(event.data.url);
          if (safeUrl) {
            openEmailLink(safeUrl);
          }
        };
        hostWindow.addEventListener("message", onFrameMessage);
        removeFrameLinkListener = () => {
          hostWindow.removeEventListener("message", onFrameMessage);
        };
        frame.srcdoc = prepareEmailHtml(selected.htmlBody);
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
        const initials = escapeHtml(
          String(mail.senderName || "?")
            .slice(0, 1)
            .toUpperCase(),
        );
        return `
          <li class="mail-item ${mail.read ? "" : "unread"} ${state.selectedId === mail.id ? "active" : ""}" data-id="${escapeHtml(mail.id)}">
            <div class="mail-item-avatar">${initials}</div>
            <div class="mail-item-content">
              <div class="mail-item-row">
                <span class="mail-item-sender">${escapeHtml(mail.senderName || "Unknown sender")}</span>
                <span class="mail-item-time">${escapeHtml(mail.time)}</span>
              </div>
              <div class="mail-item-subject">${escapeHtml(mail.subject || "(No subject)")}</div>
              <div class="mail-item-preview">${escapeHtml(mail.preview)}</div>
            </div>
          </li>
        `;
      })
      .join("");

    renderReader();
    updateCounts();
  }

  return {
    dispose,
    getVisibleEmails,
    renderReader,
    renderList,
    updateCounts,
  };
}
