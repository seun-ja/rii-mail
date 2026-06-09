import { getErrorMessage } from "./shared/errors.js";
import { tauriInvoke as invoke } from "./shared/tauri.js";

export function setLoadingState(
  loading,
  loginContentEl,
  initLoadingEl,
  initLoadingTextEl,
  message,
) {
  if (loading) {
    loginContentEl.classList.add("hidden");
    loginContentEl.setAttribute("aria-hidden", "true");
    initLoadingEl.classList.remove("hidden");
    if (message) {
      initLoadingTextEl.textContent = message;
    }
    return;
  }

  initLoadingEl.classList.add("hidden");
  loginContentEl.classList.remove("hidden");
  loginContentEl.setAttribute("aria-hidden", "false");
}

export async function checkInitializationStatus(
  loginMsgEl,
  invokeFn = invoke,
  location = window?.location,
) {
  try {
    const status = await invokeFn("check_app_status");
    const normalized = String(status || "").toLowerCase();

    if (normalized === "setup") {
      location?.replace("/setup.html");
      return true;
    }

    if (normalized === "signed_in" || normalized === "signedin") {
      location?.replace("/");
      return true;
    }

    if (normalized !== "" && normalized !== "[object object]") {
      loginMsgEl.textContent = normalized;
    }
    return false;
  } catch (_error) {
    loginMsgEl.textContent = "Could not verify app status.";
    return false;
  }
}

export async function initLoginPage({
  invokeFn = invoke,
  doc = document,
  location = window?.location,
} = {}) {
  const loginForm = doc.querySelector("#login-form");
  const loginMsgEl = doc.querySelector("#login-msg");
  const usernameInput = doc.querySelector("#username-input");
  const passwordInput = doc.querySelector("#password-input");
  const loginContentEl = doc.querySelector("#login-content");
  const initLoadingEl = doc.querySelector("#init-loading");
  const initLoadingTextEl = doc.querySelector("#init-loading-text");

  if (
    !loginForm ||
    !loginMsgEl ||
    !usernameInput ||
    !passwordInput ||
    !loginContentEl ||
    !initLoadingEl ||
    !initLoadingTextEl
  ) {
    return { ready: false, redirected: false };
  }

  if (!invokeFn) {
    setLoadingState(false, loginContentEl, initLoadingEl, initLoadingTextEl);
    loginMsgEl.textContent = "Backend connection is unavailable.";
    return { ready: false, redirected: false };
  }

  setLoadingState(
    true,
    loginContentEl,
    initLoadingEl,
    initLoadingTextEl,
    "Checking account status...",
  );

  const redirected = await checkInitializationStatus(
    loginMsgEl,
    invokeFn,
    location,
  );
  if (redirected) {
    return { ready: false, redirected: true };
  }

  setLoadingState(false, loginContentEl, initLoadingEl, initLoadingTextEl);

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

      await invokeFn("login", {
        username: username,
        password: password,
      });

      loginMsgEl.textContent = "Login successful. Opening app...";

      await invokeFn("open_main_window");
    } catch (error) {
      const errorMessage = getErrorMessage(error);
      loginMsgEl.textContent = `Login failed: ${errorMessage}`;
    }
  });

  return { ready: true, redirected: false };
}

if (typeof window !== "undefined" && typeof document !== "undefined") {
  window.addEventListener("DOMContentLoaded", async () => {
    await initLoginPage();
  });
}
