import { getErrorMessage } from "./shared/errors.js";
import { setAppSqliteDbPath } from "./shared/sqlite.js";
import { tauriInvoke as invoke } from "./shared/tauri.js";

function setLoadingState(
  loading,
  setupContentEl,
  setupLoadingEl,
  setupLoadingTextEl,
  message,
) {
  if (loading) {
    setupContentEl.classList.add("hidden");
    setupContentEl.setAttribute("aria-hidden", "true");

    setupLoadingEl.classList.remove("hidden");

    if (message) {
      setupLoadingTextEl.textContent = message;
    }

    return;
  }

  setupLoadingEl.classList.add("hidden");

  setupContentEl.classList.remove("hidden");
  setupContentEl.setAttribute("aria-hidden", "false");
}

window.addEventListener("DOMContentLoaded", () => {
  const setupForm = document.querySelector("#setup-form");
  const setupMsgEl = document.querySelector("#setup-msg");
  const providerInput = document.querySelector("#selected-provider-input");
  const imapServerInput = document.querySelector("#selected-imap-server-input");
  const imapPortInput = document.querySelector("#selected-imap-port-input");
  const setupButton = document.querySelector(".setup-button");
  const setupContentEl = document.querySelector("#setup-content");
  const setupLoadingEl = document.querySelector("#setup-loading");
  const setupLoadingTextEl = document.querySelector("#setup-loading-text");
  const providerOptions = Array.from(
    document.querySelectorAll(".provider-item"),
  );
  const setupLoadingSpinnerEl = document.querySelector(
    "#setup-loading-spinner",
  );

  if (
    !setupForm ||
    !setupMsgEl ||
    !providerInput ||
    !imapServerInput ||
    !imapPortInput ||
    !setupButton ||
    !setupContentEl ||
    !setupLoadingEl ||
    !setupLoadingTextEl ||
    providerOptions.length === 0
  ) {
    return;
  }

  if (!invoke) {
    setupMsgEl.textContent = "Backend connection is unavailable.";
    return;
  }

  const setSelectedProvider = (providerOption) => {
    // Ignore clicks on disabled options
    if (
      !providerOption ||
      providerOption.disabled ||
      providerOption.classList.contains("disabled")
    ) {
      return;
    }

    // Toggle active state for CSS highlighting
    providerOptions.forEach((option) => {
      const isSelected = option === providerOption;
      option.classList.toggle("active", isSelected);
      option.setAttribute("aria-checked", isSelected ? "true" : "false");
    });

    const nameElement = providerOption.querySelector(".custom-server-text");
    const providerName =
      providerOption.dataset.providerName ||
      (nameElement ? nameElement.textContent.trim() : "Email");

    // Update hidden inputs for backend submission
    providerInput.value = providerOption.dataset.provider || "";
    imapServerInput.value = providerOption.dataset.imapServer || "";
    imapPortInput.value = providerOption.dataset.imapPort || "";

    // Update button text dynamically based on the extracted name
    setupButton.textContent = `Continue with ${providerName}`;
  };

  // Attach click listeners to all options
  providerOptions.forEach((providerOption) => {
    providerOption.addEventListener("click", () => {
      setSelectedProvider(providerOption);
    });
  });

  // Set default active option on page load
  const defaultOption =
    providerOptions.find(
      (option) =>
        option.classList.contains("active") &&
        !option.classList.contains("disabled"),
    ) ||
    providerOptions.find((option) => !option.classList.contains("disabled"));

  setSelectedProvider(defaultOption);

  // --- Preserved Original Backend Logic ---
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
      setLoadingState(
        true,
        setupContentEl,
        setupLoadingEl,
        setupLoadingTextEl,
        "Saving configuration...",
      );

      await invoke("config_setup", {
        imapServerUrl: imapServer,
        imapPort: imapPort,
      });

      const normalizedProvider = providerInput?.value?.trim() || "yahoo";
      window.localStorage.setItem("riimail.provider", normalizedProvider);
      window.localStorage.setItem("riimail.imapServer", imapServer);
      setAppSqliteDbPath(window.localStorage);

      setupLoadingSpinnerEl?.classList.add("hidden");
      setupLoadingTextEl.textContent =
        "✓ Configuration saved. Redirecting to login...";

      setLoadingState(
        true,
        setupContentEl,
        setupLoadingEl,
        setupLoadingTextEl,
        "Configuration saved. Redirecting to login...",
      );

      // Redirect to login page after setup completes
      setTimeout(() => {
        window.location.replace("/login.html");
      }, 500);
    } catch (error) {
      setLoadingState(
        false,
        setupContentEl,
        setupLoadingEl,
        setupLoadingTextEl,
      );

      setupMsgEl.textContent = `Setup failed: ${getErrorMessage(error)}`;
    }
  });
});
