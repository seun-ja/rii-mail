export function getMailAppDom(documentRef = document) {
  return {
    folderButtons: documentRef.querySelectorAll(".folder-btn"),
    mailBodyEl: documentRef.querySelector(".mail-body"),
    mailListPanelEl: documentRef.querySelector(".mail-list-panel"),
    mailListEl: documentRef.querySelector("#mail-list"),
    mailReaderEl: documentRef.querySelector("#mail-reader"),
    sidebarToggleBtnEl: documentRef.querySelector("#sidebar-toggle-btn"),
    readerResizerEl: documentRef.querySelector("#reader-resizer"),
    searchInputEl: documentRef.querySelector("#mail-search"),
    composeBtnEl: documentRef.querySelector("#compose-btn"),
    refreshBtnEl: documentRef.querySelector("#refresh-btn"),
    spamCheckBtnEl: documentRef.querySelector("#spam-check-btn"),
    markReadBtnEl: documentRef.querySelector("#mark-read-btn"),
    archiveBtnEl: documentRef.querySelector("#archive-btn"),
    resultMsgEl: documentRef.querySelector("#result-msg"),
    syncOverlayEl: documentRef.querySelector("#sync-overlay"),
    syncMessageEl: documentRef.querySelector("#sync-message"),
    syncProgressBarEl: documentRef.querySelector("#sync-progress-bar"),
  };
}
