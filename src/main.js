import { initializeMailApp } from "./mail-app/init.js";
import { tauriInvoke, tauriListen } from "./shared/tauri.js";

window.addEventListener("DOMContentLoaded", () => {
  initializeMailApp({
    invoke: tauriInvoke,
    listen: tauriListen,
  });
});
