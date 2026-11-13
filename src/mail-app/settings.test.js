import test from "node:test";
import assert from "node:assert/strict";

import { activateSettingsSection } from "./settings.js";

function element(dataset) {
  const classes = new Set();
  const attributes = new Map();

  return {
    dataset,
    classList: {
      contains(name) {
        return classes.has(name);
      },
      toggle(name, enabled) {
        if (enabled) classes.add(name);
        else classes.delete(name);
      },
    },
    setAttribute(name, value) {
      attributes.set(name, value);
    },
    attribute(name) {
      return attributes.get(name);
    },
  };
}

test("activates the requested settings category and hides other panels", () => {
  const signatureButton = element({ settingsSection: "signature" });
  const appearanceButton = element({ settingsSection: "appearance" });
  const signaturePanel = element({ settingsPanel: "signature" });
  const appearancePanel = element({ settingsPanel: "appearance" });

  assert.equal(
    activateSettingsSection(
      {
        navButtons: [signatureButton, appearanceButton],
        panels: [signaturePanel, appearancePanel],
      },
      "appearance",
    ),
    "appearance",
  );
  assert.equal(signatureButton.classList.contains("active"), false);
  assert.equal(signatureButton.attribute("aria-selected"), "false");
  assert.equal(appearanceButton.classList.contains("active"), true);
  assert.equal(appearanceButton.attribute("aria-selected"), "true");
  assert.equal(signaturePanel.classList.contains("hidden"), true);
  assert.equal(appearancePanel.classList.contains("hidden"), false);
});

test("falls back to the first category when a section is unavailable", () => {
  const signatureButton = element({ settingsSection: "signature" });
  const signaturePanel = element({ settingsPanel: "signature" });

  assert.equal(
    activateSettingsSection(
      { navButtons: [signatureButton], panels: [signaturePanel] },
      "missing",
    ),
    "signature",
  );
  assert.equal(signatureButton.classList.contains("active"), true);
  assert.equal(signaturePanel.classList.contains("hidden"), false);
});
