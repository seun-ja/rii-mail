import test from "node:test";
import assert from "node:assert/strict";

import { initLlmSetupPage } from "../llm-setup.js";

function createElement(attributes = {}) {
  const listeners = new Map();
  const classes = new Set();

  return {
    disabled: false,
    hidden: false,
    textContent: "",
    value: "",
    classList: {
      contains(name) {
        return classes.has(name);
      },
      toggle(name, force) {
        if (force) classes.add(name);
        else classes.delete(name);
      },
    },
    focus() {
      this.focused = true;
    },
    getAttribute(name) {
      return attributes[name] ?? null;
    },
    setAttribute(name, value) {
      attributes[name] = value;
      this[`attr_${name}`] = value;
    },
    closest() {
      return this.parentOption || null;
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
  const geminiButton = createElement();
  const geminiApiKeyInput = createElement();
  const skipButton = createElement();
  const statusEl = createElement();
  const providerPanels = {
    ollama: createElement(),
    chatgpt: createElement(),
    gemini: createElement(),
  };
  Object.values(providerPanels).forEach((panel) => {
    panel.hidden = true;
  });
  providerPanels.gemini.hidden = false;
  const providerToggles = Object.keys(providerPanels).map((provider) => {
    const toggle = createElement({
      "aria-pressed": provider === "gemini" ? "true" : "false",
      "aria-controls": `${provider}-connection-panel`,
    });
    return toggle;
  });
  const elements = {
    "#connect-ollama-button": ollamaButton,
    "#connect-chatgpt-button": chatgptButton,
    "#connect-gemini-button": geminiButton,
    "#gemini-api-key": geminiApiKeyInput,
    "#skip-llm-button": skipButton,
    "#llm-setup-status": statusEl,
    "#ollama-connection-panel": providerPanels.ollama,
    "#chatgpt-connection-panel": providerPanels.chatgpt,
    "#gemini-connection-panel": providerPanels.gemini,
  };

  return {
    ollamaButton,
    chatgptButton,
    geminiButton,
    geminiApiKeyInput,
    skipButton,
    statusEl,
    providerPanels,
    providerToggles,
    doc: {
      querySelector(selector) {
        return elements[selector] || null;
      },
      querySelectorAll(selector) {
        return selector === "[data-llm-provider-choice]" ? providerToggles : [];
      },
    },
  };
}

test("provider choices show one shared connection panel at a time", async () => {
  const page = createPage();

  initLlmSetupPage({ doc: page.doc, invokeFn: async () => {} });

  await page.providerToggles[0].trigger("click");
  assert.equal(page.providerToggles[0].getAttribute("aria-pressed"), "true");
  assert.equal(page.providerPanels.ollama.hidden, false);
  assert.equal(page.providerPanels.gemini.hidden, true);

  await page.providerToggles[2].trigger("click");
  assert.equal(page.providerToggles[0].getAttribute("aria-pressed"), "false");
  assert.equal(page.providerPanels.ollama.hidden, true);
  assert.equal(page.providerToggles[2].getAttribute("aria-pressed"), "true");
  assert.equal(page.providerPanels.gemini.hidden, false);

  await page.providerToggles[2].trigger("click");
  assert.equal(page.providerToggles[2].getAttribute("aria-pressed"), "true");
  assert.equal(page.providerPanels.gemini.hidden, false);
});

test("provider details stay explorable when the backend is unavailable", async () => {
  const page = createPage();

  const result = initLlmSetupPage({ doc: page.doc, invokeFn: null });

  assert.deepEqual(result, { ready: false });
  assert.equal(page.ollamaButton.disabled, true);
  assert.equal(page.providerToggles[0].disabled, false);

  await page.providerToggles[0].trigger("click");
  assert.equal(page.providerPanels.ollama.hidden, false);
});

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

test("Gemini connection validates and sends the trimmed API key", async () => {
  const page = createPage();
  const calls = [];
  page.geminiApiKeyInput.value = "  test-gemini-key  ";

  initLlmSetupPage({
    doc: page.doc,
    invokeFn: async (command, args) => {
      calls.push([command, args]);
    },
  });

  await page.geminiButton.trigger("click");

  assert.deepEqual(calls, [
    ["connect_gemini", { apiKey: "test-gemini-key" }],
    ["open_main_window", undefined],
  ]);
  assert.equal(page.statusEl.textContent, "Connected. Opening RiiEmail...");
});

test("Gemini connection requires an API key", async () => {
  const page = createPage();
  const calls = [];

  initLlmSetupPage({
    doc: page.doc,
    invokeFn: async (command) => calls.push(command),
  });

  await page.geminiButton.trigger("click");

  assert.deepEqual(calls, []);
  assert.match(page.statusEl.textContent, /enter a Gemini API key/i);
  assert.equal(page.geminiApiKeyInput.focused, true);
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
  assert.equal(page.geminiButton.disabled, false);
  assert.equal(page.skipButton.disabled, false);
});
