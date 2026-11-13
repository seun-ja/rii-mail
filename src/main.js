import { initializeMailApp } from "./mail-app/init.js";
import { initializeComposerWindow } from "./mail-app/composer-window.js";
import { tauriInvoke, tauriListen } from "./shared/tauri.js";

async function createDetachedComposerWindow({ label, options }) {
  const WebviewWindow = window.__TAURI__?.webviewWindow?.WebviewWindow;
  const listen = window.__TAURI__?.event?.listen;

  if (!WebviewWindow) {
    const popup = window.open(
      options.url,
      label,
      `popup,width=${options.width},height=${options.height}`,
    );

    return popup
      ? Promise.resolve()
      : Promise.reject(new Error("The composer window could not be opened."));
  }

  if (typeof listen !== "function") {
    return new Promise((resolve, reject) => {
      const composerWindow = new WebviewWindow(label, options);
      void composerWindow.once("tauri://created", resolve);
      void composerWindow.once("tauri://error", (event) =>
        reject(event.payload),
      );
    });
  }

  let composerWindow;
  let resolveReady;
  let rejectReady;
  const ready = new Promise((resolve, reject) => {
    resolveReady = resolve;
    rejectReady = reject;
  });
  const unlisten = await listen("composer://ready", async (event) => {
    if (event.payload?.label !== label) return;

    try {
      await composerWindow.show();
      await composerWindow.setFocus();
      resolveReady();
    } catch (error) {
      rejectReady(error);
    }
  });

  try {
    composerWindow = new WebviewWindow(label, {
      ...options,
      visible: false,
      focus: false,
    });
    void composerWindow.once("tauri://error", (event) =>
      rejectReady(event.payload),
    );
    await ready;
  } finally {
    unlisten();
  }
}

async function closeCurrentWindow() {
  const getCurrentWebviewWindow =
    window.__TAURI__?.webviewWindow?.getCurrentWebviewWindow;

  if (getCurrentWebviewWindow) {
    await getCurrentWebviewWindow().close();
  } else {
    window.close();
  }
}

window.addEventListener("DOMContentLoaded", () => {
  if (new URLSearchParams(window.location.search).has("composerWindow")) {
    const currentWindow =
      window.__TAURI__?.webviewWindow?.getCurrentWebviewWindow?.();
    const emitToMain = (event, payload) =>
      window.__TAURI__?.event?.emitTo?.("main", event, payload);

    initializeComposerWindow({
      invoke: tauriInvoke,
      storage: window.localStorage,
      closeWindow: closeCurrentWindow,
      emitToMain,
      listen: tauriListen,
      notifyReady: () =>
        emitToMain("composer://ready", { label: currentWindow?.label }),
    });
    return;
  }

  const startupWarning = window.sessionStorage.getItem("riimail.startupWarning");
  if (startupWarning) {
    window.sessionStorage.removeItem("riimail.startupWarning");
  }

  initializeMailApp({
    invoke: tauriInvoke,
    listen: tauriListen,
    startupWarning,
    openExternalUrl: (url) =>
      tauriInvoke("plugin:opener|open_url", { url }),
    createComposerWindow: createDetachedComposerWindow,
  });
});
