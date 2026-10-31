const EMAIL_VIEWER_HEAD = `
<meta name="viewport" content="width=device-width, initial-scale=1" />
<style data-riimail-viewer-styles>
  :root {
    color-scheme: light;
    background: #ffffff;
  }

  html {
    max-width: 100%;
    background: #ffffff;
  }

  body {
    box-sizing: border-box;
    min-width: 0 !important;
    max-width: 100% !important;
    margin: 0 !important;
    padding: 18px !important;
    overflow-wrap: anywhere;
    background: #ffffff;
  }

  body > *,
  div,
  table,
  td,
  th,
  img,
  svg,
  video,
  object,
  iframe {
    max-width: 100% !important;
  }

  img,
  video {
    height: auto !important;
  }

  table {
    table-layout: auto;
  }

  pre {
    max-width: 100%;
    white-space: pre-wrap !important;
    overflow-wrap: anywhere;
  }

  a {
    overflow-wrap: anywhere;
  }

  [nowrap],
  [style*="white-space: nowrap" i],
  [style*="white-space:nowrap" i] {
    white-space: normal !important;
  }
</style>`;

export function escapeHtml(value) {
  return String(value ?? "")
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#039;");
}

export function getSafeExternalUrl(value) {
  const rawUrl = String(value ?? "").trim();
  if (!rawUrl) {
    return null;
  }

  try {
    const url = new URL(rawUrl);
    return ["http:", "https:", "mailto:"].includes(url.protocol)
      ? url.href
      : null;
  } catch {
    return null;
  }
}

export function linkifyPlainText(value) {
  const text = String(value ?? "");
  const urlPattern = /https?:\/\/[^\s<]+|www\.[^\s<]+/gi;
  let result = "";
  let lastIndex = 0;

  for (const matchResult of text.matchAll(urlPattern)) {
    const [match] = matchResult;
    const index = matchResult.index;
    result += escapeHtml(text.slice(lastIndex, index));

    let candidate = match;
    while (/[.,!?;:)\]}]$/.test(candidate)) {
      candidate = candidate.slice(0, -1);
    }

    const href = getSafeExternalUrl(
      candidate.toLowerCase().startsWith("www.")
        ? `https://${candidate}`
        : candidate,
    );
    if (!href) {
      result += escapeHtml(match);
    } else {
      result += `<a href="${escapeHtml(href)}" data-email-link>${escapeHtml(candidate)}</a>${escapeHtml(match.slice(candidate.length))}`;
    }

    lastIndex = index + match.length;
  }

  return result + escapeHtml(text.slice(lastIndex));
}

export function formatAddresses(addresses) {
  if (!Array.isArray(addresses)) {
    return [];
  }

  return addresses
    .map((entry) => {
      if (!Array.isArray(entry)) {
        return "";
      }

      const name = String(entry[0] ?? "").trim();
      const email = String(entry[1] ?? "").trim();

      if (name && email) {
        return `${name} <${email}>`;
      }

      return email || name;
    })
    .filter(Boolean);
}

export function formatAddressList(addresses) {
  return formatAddresses(addresses).join(", ");
}

export const EMAIL_LINK_MESSAGE = "riimail:open-email-link";

export function prepareEmailHtml(html) {
  const documentHtml = String(html ?? "");
  if (!documentHtml.trim()) {
    return "";
  }

  const nonce = crypto.randomUUID().replaceAll("-", "");
  // Put the policy and trusted bridge before any untrusted email markup.
  // An opaque-origin sandbox keeps even navigated email content away from the app.
  return `<!doctype html>
<html><head>
<meta http-equiv="Content-Security-Policy" content="script-src 'nonce-${nonce}'; object-src 'none'; base-uri 'none'; form-action 'none'; frame-src 'none'" />
<script nonce="${nonce}">
  document.addEventListener("click", (event) => {
    const anchor = event.target?.closest?.("a[href]");
    if (!anchor) return;
    const url = anchor.getAttribute("href");
    event.preventDefault();
    if (url?.trim().startsWith("#")) {
      // srcdoc resolves native fragment navigation against the app's URL.
      try {
        document.getElementById(decodeURIComponent(url.trim().slice(1)))?.scrollIntoView();
      } catch (_) {}
      return;
    }
    window.parent.postMessage({ type: ${JSON.stringify(EMAIL_LINK_MESSAGE)}, url }, "*");
  }, true);
</script>
</head><body>${addEmailViewerStyles(documentHtml)}</body></html>`;
}

function addEmailViewerStyles(html) {
  const documentHtml = String(html ?? "");
  if (!documentHtml.trim()) {
    return "";
  }

  if (/<\/head\s*>/i.test(documentHtml)) {
    return documentHtml.replace(/<\/head\s*>/i, `${EMAIL_VIEWER_HEAD}</head>`);
  }

  if (/<head(?:\s[^>]*)?>/i.test(documentHtml)) {
    return documentHtml.replace(
      /<head(?:\s[^>]*)?>/i,
      (headTag) => `${headTag}${EMAIL_VIEWER_HEAD}`,
    );
  }

  if (/<html(?:\s[^>]*)?>/i.test(documentHtml)) {
    return documentHtml.replace(
      /<html(?:\s[^>]*)?>/i,
      (htmlTag) => `${htmlTag}<head>${EMAIL_VIEWER_HEAD}</head>`,
    );
  }

  return `${EMAIL_VIEWER_HEAD}${documentHtml}`;
}

export function delay(ms) {
  return new Promise((resolve) => {
    window.setTimeout(resolve, ms);
  });
}

export function getActiveBackendFolderKey(state) {
  if (state.activeFolder === "Sent") {
    return "Sent";
  }

  if (state.activeFolder === "Trash") {
    return "Trash";
  }

  return "INBOX";
}

export function getActiveMailboxLiteral(state) {
  return getActiveBackendFolderKey(state);
}

export function getFolderPaginationState(
  state,
  folderKey = getActiveBackendFolderKey(state),
) {
  return state.paginationByFolder[folderKey];
}

export function resetFolderPagination(state, folderKey) {
  const pagination = state.paginationByFolder[folderKey];
  if (!pagination) {
    return;
  }

  pagination.nextOffset = 0;
  pagination.hasMoreEmails = true;
  pagination.lastFetchSignature = null;
  pagination.lastFetchAt = 0;
}

export function isMailListNearBottom(mailListEl, threshold = 40) {
  if (!mailListEl) {
    return false;
  }

  return (
    mailListEl.scrollTop + mailListEl.clientHeight >=
    mailListEl.scrollHeight - threshold
  );
}

function prefixedSubject(subject, prefix) {
  const normalizedSubject = String(subject || "(No subject)").trim();
  return /^(?:re|fw|fwd)\s*:/i.test(normalizedSubject)
    ? normalizedSubject
    : `${prefix}: ${normalizedSubject}`;
}

function getPlainMessageBody(email) {
  const body = String(email?.textBody || email?.body || email?.htmlBody || "");

  return body
    .replace(/<\s*br\s*\/?\s*>/gi, "\n")
    .replace(/<\/(?:p|div|li|tr|h[1-6])\s*>/gi, "\n")
    .replace(/<[^>]+>/g, "")
    .replace(/&nbsp;/gi, " ")
    .replace(/&amp;/gi, "&")
    .replace(/&lt;/gi, "<")
    .replace(/&gt;/gi, ">")
    .replace(/&quot;/gi, '"')
    .replace(/&#039;/gi, "'")
    .trim();
}

function getSenderLabel(email) {
  const name = String(email?.senderName || "").trim();
  const address = String(email?.emailFrom || "").trim();

  if (name && address) {
    return `${name} <${address}>`;
  }

  return name || address || "Unknown sender";
}

export function createReplyDraft(email) {
  const sender = getSenderLabel(email);
  const sentAt = String(email?.time || "Unknown date").trim();
  const quotedBody = getPlainMessageBody(email)
    .split("\n")
    .map((line) => `> ${line}`)
    .join("\n");

  return {
    to: String(email?.emailFrom || "").trim(),
    subject: prefixedSubject(email?.subject, "Re"),
    body: `\n\nOn ${sentAt}, ${sender} wrote:\n${quotedBody}`,
  };
}

export function createForwardDraft(email) {
  const sender = getSenderLabel(email);
  const sentAt = String(email?.time || "Unknown date").trim();
  const subject = String(email?.subject || "(No subject)").trim();
  const cc = formatAddressList(email?.cc);
  const ccHeader = cc ? `\nCc: ${cc}` : "";

  return {
    to: "",
    subject: prefixedSubject(subject, "Fwd"),
    body: `\n\n---------- Forwarded message ---------\nFrom: ${sender}\nDate: ${sentAt}${ccHeader}\nSubject: ${subject}\n\n${getPlainMessageBody(email)}`,
  };
}
