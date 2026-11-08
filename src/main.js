import { initializeMailApp } from "./mail-app/init.js";
import { initializeComposerWindow } from "./mail-app/composer-window.js";
import { tauriInvoke, tauriListen } from "./shared/tauri.js";

function createDetachedComposerWindow({ label, options }) {
  const WebviewWindow = window.__TAURI__?.webviewWindow?.WebviewWindow;

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

  return new Promise((resolve, reject) => {
    const composerWindow = new WebviewWindow(label, options);
    void composerWindow.once("tauri://created", resolve);
    void composerWindow.once("tauri://error", (event) => reject(event.payload));
  });
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
    initializeComposerWindow({
      invoke: tauriInvoke,
      storage: window.localStorage,
      closeWindow: closeCurrentWindow,
      emitToMain: (event, payload) =>
        window.__TAURI__?.event?.emitTo?.("main", event, payload),
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
