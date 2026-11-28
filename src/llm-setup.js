import { getErrorMessage } from "./shared/errors.js";
import { tauriInvoke as invoke } from "./shared/tauri.js";

function setBusy(buttons, statusEl, busy, message) {
  buttons.forEach((button) => {
    button.disabled = busy;
  });
  statusEl.setAttribute("aria-busy", busy ? "true" : "false");
  statusEl.textContent = message;
}

export function initLlmSetupPage({
  invokeFn = invoke,
  doc = document,
} = {}) {
  const ollamaButton = doc.querySelector("#connect-ollama-button");
  const chatgptButton = doc.querySelector("#connect-chatgpt-button");
  const geminiButton = doc.querySelector("#connect-gemini-button");
  const geminiApiKeyInput = doc.querySelector("#gemini-api-key");
  const skipButton = doc.querySelector("#skip-llm-button");
  const statusEl = doc.querySelector("#llm-setup-status");
  const providerChoices = Array.from(
    doc.querySelectorAll?.("[data-llm-provider-choice]") || [],
  );

  if (
    !ollamaButton ||
    !chatgptButton ||
    !geminiButton ||
    !geminiApiKeyInput ||
    !skipButton ||
    !statusEl ||
    providerChoices.length === 0
  ) {
    return { ready: false };
  }

  const connectionButtons = [
    ollamaButton,
    chatgptButton,
    geminiButton,
    skipButton,
  ];
  const buttons = [...providerChoices, ...connectionButtons];

  let isBusy = false;

  const selectProvider = (selectedChoice) => {
    if (isBusy) {
      return;
    }

    providerChoices.forEach((choice) => {
      const isSelected = choice === selectedChoice;
      const panelId = choice.getAttribute("aria-controls");
      const panel = panelId ? doc.querySelector(`#${panelId}`) : null;

      choice.setAttribute("aria-pressed", isSelected ? "true" : "false");
      if (panel) {
        panel.hidden = !isSelected;
      }
    });
  };

  providerChoices.forEach((choice) => {
    choice.addEventListener("click", () => selectProvider(choice));
  });

  selectProvider(
    providerChoices.find((choice) => choice.getAttribute("aria-pressed") === "true") ||
      providerChoices[0],
  );

  if (!invokeFn) {
    setBusy(
      connectionButtons,
      statusEl,
      true,
      "Backend connection is unavailable. Please restart RiiEmail.",
    );
    return { ready: false };
  }

  const runAction = async (command, pendingMessage, args) => {
    if (isBusy) {
      return;
    }

    isBusy = true;
    setBusy(buttons, statusEl, true, pendingMessage);

    try {
      if (command) {
        await invokeFn(command, args);
        statusEl.textContent = "Connected. Opening RiiEmail...";
      }

      await invokeFn("open_main_window");
    } catch (error) {
      isBusy = false;
      setBusy(
        buttons,
        statusEl,
        false,
        `Could not complete AI setup: ${getErrorMessage(error)}`,
      );
    }
  };

  ollamaButton.addEventListener("click", () =>
    runAction("connect_ollama", "Connecting to Ollama..."),
  );
  chatgptButton.addEventListener("click", () =>
    runAction(
      "connect_chatgpt",
      "Opening your system browser. Complete sign-in there; RiiEmail will wait here...",
    ),
  );
  geminiButton.addEventListener("click", () => {
    const apiKey = geminiApiKeyInput.value.trim();
    if (!apiKey) {
      statusEl.textContent = "Enter a Gemini API key from Google AI Studio.";
      geminiApiKeyInput.focus();
      return;
    }
    runAction("connect_gemini", "Checking your Gemini API key...", { apiKey });
  });
  skipButton.addEventListener("click", () =>
    runAction(null, "Opening RiiEmail..."),
  );

  return { ready: true };
}

if (typeof window !== "undefined" && typeof document !== "undefined") {
  window.addEventListener("DOMContentLoaded", () => {
    initLlmSetupPage();
  });
}
