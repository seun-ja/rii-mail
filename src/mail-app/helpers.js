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

export function prepareEmailHtml(html) {
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
