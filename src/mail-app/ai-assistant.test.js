import test from "node:test";
import assert from "node:assert/strict";

import { updateAiAssistantAvailability } from "./ai-assistant.js";

function createElement() {
  const classes = new Set();
  return {
    classList: {
      add(name) {
        classes.add(name);
      },
      contains(name) {
        return classes.has(name);
      },
      toggle(name, enabled) {
        if (enabled) {
          classes.add(name);
        } else {
          classes.delete(name);
        }
      },
    },
  };
}

test("shows the compose AI button when an LLM is enabled", async () => {
  const button = createElement();
  const panel = createElement();
  const calls = [];

  const enabled = await updateAiAssistantAvailability({
    button,
    panel,
    invoke: async (command) => {
      calls.push(command);
      return true;
    },
  });

  assert.equal(enabled, true);
  assert.deepEqual(calls, ["is_llm_enabled"]);
  assert.equal(button.classList.contains("hidden"), false);
  assert.equal(panel.classList.contains("hidden"), true);
});

test("keeps the compose AI controls hidden when setup was skipped", async () => {
  const button = createElement();
  const panel = createElement();

  const enabled = await updateAiAssistantAvailability({
    button,
    panel,
    invoke: async () => false,
  });

  assert.equal(enabled, false);
  assert.equal(button.classList.contains("hidden"), true);
  assert.equal(panel.classList.contains("hidden"), true);
});

test("keeps the compose AI controls hidden when provider state is unavailable", async () => {
  const button = createElement();

  const enabled = await updateAiAssistantAvailability({
    button,
    panel: createElement(),
    invoke: async () => {
      throw new Error("backend unavailable");
    },
  });

  assert.equal(enabled, false);
  assert.equal(button.classList.contains("hidden"), true);
});
