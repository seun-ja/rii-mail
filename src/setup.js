import { getErrorMessage } from "./shared/errors.js";
import { tauriInvoke as invoke } from "./shared/tauri.js";

window.addEventListener("DOMContentLoaded", () => {
  const setupForm = document.querySelector("#setup-form");
  const setupMsgEl = document.querySelector("#setup-msg");
  const imapServerInput = document.querySelector("#imap-server-input");
  const imapPortInput = document.querySelector("#imap-port-input");

  if (!setupForm || !setupMsgEl || !imapServerInput || !imapPortInput) {
    return;
  }

  if (!invoke) {
    setupMsgEl.textContent = "Backend connection is unavailable.";
    return;
  }

  setupForm.addEventListener("submit", async (event) => {
    event.preventDefault();

    const imapServer = imapServerInput?.value?.trim() || "";
    const imapPortValue = imapPortInput?.value?.trim() || "";

    const imapPort = Number.parseInt(imapPortValue, 10);

    if (Number.isNaN(imapPort)) {
      setupMsgEl.textContent = "IMAP port must be a valid number.";
      return;
    }

    try {
      setupMsgEl.textContent = "Saving configuration...";

      await invoke("config_setup", {
        imapServer: imapServer,
        imapPort: imapPort,
      });

      const normalizedProvider = imapServer.toLowerCase().includes("yahoo")
        ? "yahoo"
        : "gmail";
      window.localStorage.setItem("pemail.provider", normalizedProvider);
      window.localStorage.setItem("pemail.imapServer", imapServer);

      setupMsgEl.textContent = "Configuration saved. Redirecting to login...";

      // Redirect to login page after setup completes
      setTimeout(() => {
        window.location.replace("/login.html");
      }, 500);
    } catch (error) {
      setupMsgEl.textContent = `Setup failed: ${getErrorMessage(error)}`;
    }
  });
});
