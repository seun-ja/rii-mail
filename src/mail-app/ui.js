export function createUiController(dom) {
  let toastTimer = null;

  function showMessage(text, isError = false) {
    if (!dom.resultMsgEl) {
      return;
    }

    dom.resultMsgEl.textContent = text;
    dom.resultMsgEl.classList.add("show");
    dom.resultMsgEl.classList.toggle("error", isError);

    if (toastTimer) {
      clearTimeout(toastTimer);
    }

    toastTimer = setTimeout(() => {
      dom.resultMsgEl?.classList.remove("show");
    }, 2800);
  }

  function setSyncUiState(locked, message = "Syncing...", percent = 0) {
    if (!dom.syncOverlayEl || !dom.syncMessageEl || !dom.syncProgressBarEl) {
      return;
    }

    const safePercent = Math.max(0, Math.min(100, percent));

    dom.syncOverlayEl.classList.toggle("hidden", !locked);
    dom.syncMessageEl.textContent = message;
    dom.syncProgressBarEl.style.width = `${safePercent}%`;
    dom.syncOverlayEl
      .querySelector("[role='progressbar']")
      ?.setAttribute("aria-valuenow", String(Math.round(safePercent)));

    document
      .querySelector(".mail-app")
      ?.classList.toggle("sync-locked", locked);
  }

  function updateDbPopulation(inbox, sent) {
    const container = document.getElementById("db-population");
    const bar = document.getElementById("db-population-bar");
    const text = document.getElementById("db-population-text");

    if (!container || !bar || !text) {
      return;
    }

    const value = Math.max(inbox, sent);

    bar.style.width = `${value}%`;
    text.textContent = `Building local mailbox... ${value}%`;

    if (inbox >= 100 && sent >= 100) {
      container.classList.add("hidden");
    } else {
      container.classList.remove("hidden");
    }
  }

  return {
    showMessage,
    setSyncUiState,
    updateDbPopulation,
  };
}
