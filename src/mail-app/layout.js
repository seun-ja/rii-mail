export function createLayoutController(state, dom) {
  function setReaderListWidth(nextWidthPx) {
    if (!dom.mailBodyEl) {
      return;
    }

    const bodyRect = dom.mailBodyEl.getBoundingClientRect();
    const sidebarWidth = state.isSidebarCollapsed ? 74 : 220;
    const resizerWidth = 10;
    const minListWidth = 260;
    const minReaderWidth = 360;
    const maxListWidth = Math.max(
      minListWidth,
      bodyRect.width - sidebarWidth - resizerWidth - minReaderWidth,
    );
    const clamped = Math.max(minListWidth, Math.min(nextWidthPx, maxListWidth));

    dom.mailBodyEl.style.setProperty(
      "--list-panel-width",
      `${Math.round(clamped)}px`,
    );
  }

  function syncLayoutState() {
    if (!dom.mailBodyEl) {
      return;
    }

    dom.mailBodyEl.classList.toggle(
      "sidebar-collapsed",
      state.isSidebarCollapsed,
    );

    if (dom.sidebarToggleBtnEl) {
      dom.sidebarToggleBtnEl.setAttribute(
        "aria-expanded",
        String(!state.isSidebarCollapsed),
      );
      dom.sidebarToggleBtnEl.setAttribute(
        "aria-label",
        state.isSidebarCollapsed
          ? "Expand mailbox section"
          : "Collapse mailbox section",
      );
      dom.sidebarToggleBtnEl.classList.toggle(
        "is-collapsed",
        state.isSidebarCollapsed,
      );
    }

    if (window.innerWidth > 980 && dom.mailListPanelEl) {
      setReaderListWidth(dom.mailListPanelEl.getBoundingClientRect().width);
    }
  }

  function onReaderResizeMove(event) {
    if (!state.isResizingReader || !dom.mailBodyEl) {
      return;
    }

    const bodyRect = dom.mailBodyEl.getBoundingClientRect();
    const sidebarWidth = state.isSidebarCollapsed ? 74 : 220;
    const relativeX = event.clientX - bodyRect.left - sidebarWidth;

    setReaderListWidth(relativeX);
  }

  function onReaderResizeEnd() {
    if (!state.isResizingReader) {
      return;
    }

    state.isResizingReader = false;
    document.body.style.cursor = "";
    dom.readerResizerEl?.classList.remove("is-dragging");
    window.removeEventListener("pointermove", onReaderResizeMove);
    window.removeEventListener("pointerup", onReaderResizeEnd);
    window.removeEventListener("pointercancel", onReaderResizeEnd);
  }

  function setupLayoutEvents() {
    if (dom.sidebarToggleBtnEl) {
      dom.sidebarToggleBtnEl.addEventListener("click", () => {
        state.isSidebarCollapsed = !state.isSidebarCollapsed;
        syncLayoutState();
      });
    }

    if (dom.readerResizerEl) {
      dom.readerResizerEl.addEventListener("pointerdown", (event) => {
        if (window.innerWidth <= 980) {
          return;
        }

        event.preventDefault();
        state.isResizingReader = true;
        document.body.style.cursor = "col-resize";
        dom.readerResizerEl.classList.add("is-dragging");

        window.addEventListener("pointermove", onReaderResizeMove);
        window.addEventListener("pointerup", onReaderResizeEnd);
        window.addEventListener("pointercancel", onReaderResizeEnd);
      });
    }

    window.addEventListener("resize", () => {
      if (window.innerWidth <= 980) {
        return;
      }

      if (dom.mailListPanelEl) {
        setReaderListWidth(dom.mailListPanelEl.getBoundingClientRect().width);
      }
    });
  }

  return {
    syncLayoutState,
    setupLayoutEvents,
  };
}
