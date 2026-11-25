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

  if (
    !ollamaButton ||
    !chatgptButton ||
    !geminiButton ||
    !geminiApiKeyInput ||
    !skipButton ||
    !statusEl
  ) {
    return { ready: false };
  }

  const buttons = [ollamaButton, chatgptButton, geminiButton, skipButton];

  if (!invokeFn) {
    setBusy(
      buttons,
      statusEl,
      true,
      "Backend connection is unavailable. Please restart RiiEmail.",
    );
    return { ready: false };
  }

  let isBusy = false;

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
