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
