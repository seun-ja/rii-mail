const { invoke } = window.__TAURI__.core;

async function checkAlreadyInitialized() {
  try {
    const status = await invoke("check_init_status");
    const isSignedIn = status === "signed_in" || status === "signedin";

    if (isSignedIn) {
      // User already signed in, redirect to main app
      window.location.replace("/");
    } else if (status === "login") {
      // Config exists but not signed in, redirect to login page
      window.location.replace("/login.html");
    }
    // If status === "setup", we're on the right page, continue
  } catch (error) {
    setupMsgEl.textContent = `Could not check initialization status`;
  }
}

window.addEventListener("DOMContentLoaded", () => {
  const setupForm = document.querySelector("#setup-form");
  const setupMsgEl = document.querySelector("#setup-msg");

  checkAlreadyInitialized();

  setupForm.addEventListener("submit", async (event) => {
    event.preventDefault();

    const imapServerInput = document.querySelector("#imap-server-input");
    const imapPortInput = document.querySelector("#imap-port-input");

    const imapServer = imapServerInput?.value?.trim() || "";
    const imapPortValue = imapPortInput?.value?.trim() || "";

    const imapPort = Number.parseInt(imapPortValue, 10);

    if (Number.isNaN(imapPort)) {
      setupMsgEl.textContent = "IMAP port must be a valid number.";
      return;
    }

    try {
      await invoke("config_setup", {
        imapServer: imapServer,
        imapPort: imapPort,
      });
      setupMsgEl.textContent = "Configuration saved. Redirecting to login...";

      // Redirect to login page after setup completes
      setTimeout(() => {
        window.location.replace("/login.html");
      }, 500);
    } catch (error) {
      let errorMessage = "Unknown error";
      if (error instanceof Error) {
        errorMessage = error.message;
      } else if (typeof error === "string") {
        errorMessage = error;
      } else if (error && typeof error === "object") {
        errorMessage = error.message || error.msg || JSON.stringify(error);
      }
    }
  });
});
