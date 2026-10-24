import test from "node:test";
import assert from "node:assert/strict";

import { initLlmSetupPage } from "../llm-setup.js";

function createElement() {
  const listeners = new Map();

  return {
    disabled: false,
    textContent: "",
    setAttribute(name, value) {
      this[`attr_${name}`] = value;
    },
    addEventListener(name, handler) {
      listeners.set(name, handler);
    },
    async trigger(name) {
      const handler = listeners.get(name);
      if (handler) {
        await handler();
      }
    },
  };
}

function createPage() {
  const ollamaButton = createElement();
  const chatgptButton = createElement();
  const skipButton = createElement();
  const statusEl = createElement();
  const elements = {
    "#connect-ollama-button": ollamaButton,
    "#connect-chatgpt-button": chatgptButton,
    "#skip-llm-button": skipButton,
    "#llm-setup-status": statusEl,
  };

  return {
    ollamaButton,
    chatgptButton,
    skipButton,
    statusEl,
    doc: {
      querySelector(selector) {
        return elements[selector] || null;
      },
    },
  };
}

test("Ollama connection opens the main window after connecting", async () => {
  const page = createPage();
  const calls = [];
  const result = initLlmSetupPage({
    doc: page.doc,
    invokeFn: async (command) => {
      calls.push(command);
    },
  });

  assert.deepEqual(result, { ready: true });
  await page.ollamaButton.trigger("click");

  assert.deepEqual(calls, ["connect_ollama", "open_main_window"]);
  assert.equal(page.statusEl.textContent, "Connected. Opening RiiEmail...");
});

test("ChatGPT connection explains browser authorization while awaiting the command", async () => {
  const page = createPage();
  const calls = [];
  let completeAuthorization;
  const authorizationPending = new Promise((resolve) => {
    completeAuthorization = resolve;
  });

  initLlmSetupPage({
    doc: page.doc,
    invokeFn: async (command) => {
      calls.push(command);
      if (command === "connect_chatgpt") {
        await authorizationPending;
      }
    },
  });

  const clickPromise = page.chatgptButton.trigger("click");
  await Promise.resolve();

  assert.deepEqual(calls, ["connect_chatgpt"]);
  assert.match(page.statusEl.textContent, /system browser/i);
  assert.match(page.statusEl.textContent, /wait here/i);
  assert.equal(page.statusEl["attr_aria-busy"], "true");
  assert.equal(page.chatgptButton.disabled, true);
  assert.equal(page.ollamaButton.disabled, true);
  assert.equal(page.skipButton.disabled, true);

  completeAuthorization();
  await clickPromise;

  assert.deepEqual(calls, ["connect_chatgpt", "open_main_window"]);
});

test("Skip opens the main window without connecting a provider", async () => {
  const page = createPage();
  const calls = [];

  initLlmSetupPage({
    doc: page.doc,
    invokeFn: async (command) => {
      calls.push(command);
    },
  });

  await page.skipButton.trigger("click");

  assert.deepEqual(calls, ["open_main_window"]);
  assert.equal(page.statusEl.textContent, "Opening RiiEmail...");
});

test("failed connections show an accessible error and restore the actions", async () => {
  const page = createPage();

  initLlmSetupPage({
    doc: page.doc,
    invokeFn: async () => {
      throw new Error("Ollama is not running");
    },
  });

  await page.ollamaButton.trigger("click");

  assert.equal(
    page.statusEl.textContent,
    "Could not complete AI setup: Ollama is not running",
  );
  assert.equal(page.statusEl["attr_aria-busy"], "false");
  assert.equal(page.ollamaButton.disabled, false);
  assert.equal(page.chatgptButton.disabled, false);
  assert.equal(page.skipButton.disabled, false);
});
