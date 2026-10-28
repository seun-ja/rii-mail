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
  const setupButtonLabel = setupButton?.querySelector("span");
  const customFieldsEl = document.querySelector("#custom-imap-fields");
  const customServerInput = document.querySelector("#custom-imap-server");
  const customPortInput = document.querySelector("#custom-imap-port");
  const customSmtpFieldEl = document.querySelector("#custom-smtp-field");
  const customSmtpInput = document.querySelector("#custom-smtp-server");
  const setupContentEl = document.querySelector("#setup-content");
  const setupLoadingEl = document.querySelector("#setup-loading");
  const setupLoadingTextEl = document.querySelector("#setup-loading-text");
  const providerOptions = Array.from(
    document.querySelectorAll("button.provider-item"),
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
    !setupButtonLabel ||
    !customFieldsEl ||
    !customServerInput ||
    !customPortInput ||
    !customSmtpFieldEl ||
    !customSmtpInput ||
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

  const setCustomSmtpFieldVisible = (visible) => {
    customSmtpFieldEl.classList.toggle("hidden", !visible);
    customSmtpFieldEl.setAttribute("aria-hidden", visible ? "false" : "true");
    customSmtpInput.required = visible;

    if (!visible) {
      customSmtpInput.value = "";
    }
  };

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

      if (option.dataset.provider === "custom") {
        option.setAttribute(
          "aria-expanded",
          isSelected ? "true" : "false",
        );
      }
    });

    const nameElement = providerOption.querySelector(".provider-name");
    const providerName =
      providerOption.dataset.providerName ||
      (nameElement ? nameElement.textContent.trim() : "Email");
    const provider = providerOption.dataset.provider || "";
    const isCustomProvider = provider === "custom";

    customFieldsEl.classList.toggle("hidden", !isCustomProvider);
    customFieldsEl.setAttribute(
      "aria-hidden",
      isCustomProvider ? "false" : "true",
    );
    customServerInput.required = isCustomProvider;
    customPortInput.required = isCustomProvider;

    if (!isCustomProvider) {
      setCustomSmtpFieldVisible(false);
    }

    // Update hidden inputs for backend submission.
    providerInput.value = provider;
    imapServerInput.value = isCustomProvider
      ? customServerInput.value.trim()
      : providerOption.dataset.imapServer || "";
    imapPortInput.value = isCustomProvider
      ? customPortInput.value.trim()
      : providerOption.dataset.imapPort || "";

    setupMsgEl.textContent = "";
    setupButtonLabel.textContent = `Continue with ${providerName}`;
  };

  // Attach click listeners to all options
  providerOptions.forEach((providerOption) => {
    providerOption.addEventListener("click", () => {
      setSelectedProvider(providerOption);
    });
  });

  const syncCustomFields = () => {
    if (providerInput.value !== "custom") {
      return;
    }

    imapServerInput.value = customServerInput.value.trim();
    imapPortInput.value = customPortInput.value.trim();
    setCustomSmtpFieldVisible(false);
    setupButtonLabel.textContent = "Continue with Custom IMAP server";
    setupMsgEl.textContent = "";
  };

  customServerInput.addEventListener("input", syncCustomFields);
  customPortInput.addEventListener("input", syncCustomFields);

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
    const smtpRelayUrl = customSmtpInput.value.trim() || null;

    const imapPort = Number.parseInt(imapPortValue, 10);

    if (!imapServer) {
      setupMsgEl.textContent = "Enter the IMAP server address.";
      return;
    }

    if (Number.isNaN(imapPort) || imapPort < 1 || imapPort > 65535) {
      setupMsgEl.textContent = "IMAP port must be between 1 and 65535.";
      return;
    }

    try {
      setLoadingState(
        true,
        setupContentEl,
        setupLoadingEl,
        setupLoadingTextEl,
        smtpRelayUrl
          ? "Saving configuration..."
          : "Discovering SMTP server...",
      );

      await invoke("config_setup", {
        imapServerUrl: imapServer,
        imapPort: imapPort,
        smtpRelayUrl,
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

      const errorMessage = getErrorMessage(error);
      const needsCustomSmtpServer =
        providerInput.value === "custom" &&
        (error?.kind === "SmtpDiscovery" ||
          errorMessage.includes("No reachable SMTP server"));

      if (needsCustomSmtpServer) {
        setCustomSmtpFieldVisible(true);
        setupButtonLabel.textContent = "Continue with SMTP server";
        setupMsgEl.textContent = errorMessage;
        customSmtpInput.focus();
        return;
      }

      setupMsgEl.textContent = `Setup failed: ${errorMessage}`;
    }
  });
});
