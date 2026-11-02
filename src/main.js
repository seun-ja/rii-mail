import { initializeMailApp } from "./mail-app/init.js";
import { tauriInvoke, tauriListen } from "./shared/tauri.js";

window.addEventListener("DOMContentLoaded", () => {
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
  });
});
