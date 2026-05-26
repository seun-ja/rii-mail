import { getErrorMessage } from "./shared/errors.js";
import { tauriInvoke as invoke } from "./shared/tauri.js";

window.addEventListener("DOMContentLoaded", () => {
  const loginForm = document.querySelector("#login-form");
  const loginMsgEl = document.querySelector("#login-msg");
  const usernameInput = document.querySelector("#username-input");
  const passwordInput = document.querySelector("#password-input");

  if (!loginForm || !loginMsgEl || !usernameInput || !passwordInput) {
    return;
  }

  if (!invoke) {
    loginMsgEl.textContent = "Backend connection is unavailable.";
    return;
  }

  loginForm.addEventListener("submit", async (event) => {
    event.preventDefault();

    const username = usernameInput?.value?.trim() || "";
    const password = passwordInput?.value?.trim() || "";

    if (!username || !password) {
      loginMsgEl.textContent = "Please enter both username and password.";
      return;
    }

    try {
      loginMsgEl.textContent = "Logging in...";

      await invoke("login", {
        username: username,
        password: password,
      });

      loginMsgEl.textContent = "Login successful. Opening app...";

      await invoke("open_main_window");
    } catch (error) {
      const errorMessage = getErrorMessage(error);
      loginMsgEl.textContent = `Login failed: ${errorMessage}`;
    }
  });
});
