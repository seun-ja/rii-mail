const { invoke } = window.__TAURI__.core;

window.addEventListener("DOMContentLoaded", () => {
  const loginForm = document.querySelector("#login-form");
  const loginMsgEl = document.querySelector("#login-msg");

  loginForm.addEventListener("submit", async (event) => {
    event.preventDefault();

    const usernameInput = document.querySelector("#username-input");
    const passwordInput = document.querySelector("#password-input");

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
      let errorMessage = "Unknown error";
      if (error instanceof Error) {
        errorMessage = error.message;
      } else if (typeof error === "string") {
        errorMessage = error;
      } else if (error && typeof error === "object") {
        // Handle Rust Error struct serialized as object with 'message' field
        errorMessage = error.message || error.msg || JSON.stringify(error);
      }

      loginMsgEl.textContent = `Login failed: ${errorMessage}`;
    }
  });
});
