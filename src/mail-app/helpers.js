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
