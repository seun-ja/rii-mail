import { tauriInvoke as invoke } from "./shared/tauri.js";

async function finishStartup() {
  if (!invoke) {
    console.error("Backend connection is unavailable during startup.");
    return;
  }

  try {
    await invoke("finish_startup");
  } catch (error) {
    console.error("Could not finish RiiMail startup:", error);
  }
}

window.addEventListener("DOMContentLoaded", finishStartup, { once: true });
