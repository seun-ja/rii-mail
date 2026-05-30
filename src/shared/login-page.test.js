import test from "node:test";
import assert from "node:assert/strict";

import {
  checkInitializationStatus,
  initLoginPage,
  setLoadingState,
} from "../login.js";

function createElement(initialText = "") {
  const listeners = new Map();
  const classSet = new Set();

  return {
    textContent: initialText,
    value: "",
    classList: {
      add(name) {
        classSet.add(name);
      },
      remove(name) {
        classSet.delete(name);
      },
      contains(name) {
        return classSet.has(name);
      },
    },
    setAttribute(name, value) {
      this[`attr_${name}`] = value;
    },
    addEventListener(name, handler) {
      listeners.set(name, handler);
    },
    async trigger(name, event = {}) {
      const handler = listeners.get(name);
      if (!handler) {
        return;
      }
      await handler(event);
    },
  };
}

function createDoc(elements) {
  return {
    querySelector(selector) {
      return elements[selector] || null;
    },
  };
}

test("setLoadingState toggles login loading visibility", () => {
  const loginContentEl = createElement();
  const initLoadingEl = createElement();
  const initLoadingTextEl = createElement();

  setLoadingState(
    true,
    loginContentEl,
    initLoadingEl,
    initLoadingTextEl,
    "Checking account status...",
  );

  assert.equal(loginContentEl.classList.contains("hidden"), true);
  assert.equal(initLoadingEl.classList.contains("hidden"), false);
  assert.equal(initLoadingTextEl.textContent, "Checking account status...");

  setLoadingState(false, loginContentEl, initLoadingEl, initLoadingTextEl);

  assert.equal(loginContentEl.classList.contains("hidden"), false);
  assert.equal(initLoadingEl.classList.contains("hidden"), true);
  assert.equal(loginContentEl["attr_aria-hidden"], "false");
});

test("checkInitializationStatus redirects on setup", async () => {
  const loginMsgEl = createElement();
  let redirectedTo = "";

  const redirected = await checkInitializationStatus(
    loginMsgEl,
    async (command) => {
      assert.equal(command, "check_app_status");
      return "setup";
    },
    {
      replace(path) {
        redirectedTo = path;
      },
    },
  );

  assert.equal(redirected, true);
  assert.equal(redirectedTo, "/setup.html");
});

test("initLoginPage reveals login when status is login and validates empty credentials", async () => {
  const loginForm = createElement();
  const loginMsgEl = createElement();
  const usernameInput = createElement();
  const passwordInput = createElement();
  const loginContentEl = createElement();
  const initLoadingEl = createElement();
  const initLoadingTextEl = createElement();

  const doc = createDoc({
    "#login-form": loginForm,
    "#login-msg": loginMsgEl,
    "#username-input": usernameInput,
    "#password-input": passwordInput,
    "#login-content": loginContentEl,
    "#init-loading": initLoadingEl,
    "#init-loading-text": initLoadingTextEl,
  });

  const callLog = [];

  const initResult = await initLoginPage({
    doc,
    location: { replace() {} },
    invokeFn: async (command) => {
      callLog.push(command);
      if (command === "check_app_status") {
        return "login";
      }
      return null;
    },
  });

  assert.deepEqual(initResult, { ready: true, redirected: false });
  assert.equal(loginContentEl.classList.contains("hidden"), false);
  assert.equal(initLoadingEl.classList.contains("hidden"), true);

  await loginForm.trigger("submit", {
    preventDefault() {},
  });

  assert.equal(
    loginMsgEl.textContent,
    "Please enter both username and password.",
  );
  assert.deepEqual(callLog, ["check_app_status"]);
});
