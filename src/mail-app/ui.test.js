import test from "node:test";
import assert from "node:assert/strict";
import { createUiController } from "./ui.js";

function createToast() {
  const classes = new Set();
  const title = { textContent: "" };
  const message = { textContent: "" };
  let dismiss;
  const close = {
    addEventListener(event, listener) {
      assert.equal(event, "click");
      dismiss = listener;
    },
  };
  const elements = {
    ".status-toast-title": title,
    ".status-toast-message": message,
    ".status-toast-close": close,
  };
  const toast = {
    inert: true,
    querySelector: (selector) => elements[selector],
    classList: {
      add: (name) => classes.add(name),
      remove: (name) => classes.delete(name),
      toggle(name, enabled) {
        if (enabled) classes.add(name);
        else classes.delete(name);
      },
      contains: (name) => classes.has(name),
    },
  };
  return { toast, title, message, dismiss: () => dismiss() };
}

test("error notifications use a heading and preserve message text safely", (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const { toast, title, message } = createToast();
  const ui = createUiController({ resultMsgEl: toast });

  ui.showMessage("Could not connect. <script>not markup</script>", true);

  assert.equal(title.textContent, "Something went wrong");
  assert.equal(message.textContent, "Could not connect. <script>not markup</script>");
  assert.equal(toast.classList.contains("error"), true);
  assert.equal(toast.classList.contains("show"), true);
  assert.equal(toast.inert, false);
  t.mock.timers.tick(7999);
  assert.equal(toast.classList.contains("show"), true);
  t.mock.timers.tick(1);
  assert.equal(toast.classList.contains("show"), false);
  assert.equal(toast.inert, true);
});

test("a normal update replaces the error state and resets its dismissal timer", (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const { toast, title, message } = createToast();
  const ui = createUiController({ resultMsgEl: toast });

  ui.showMessage("Connection failed", true);
  t.mock.timers.tick(7000);
  ui.showMessage("Mailbox refreshed.");
  assert.equal(title.textContent, "Mailbox update");
  assert.equal(message.textContent, "Mailbox refreshed.");
  assert.equal(toast.classList.contains("error"), false);
  t.mock.timers.tick(1000);
  assert.equal(toast.classList.contains("show"), true);
  t.mock.timers.tick(1800);
  assert.equal(toast.classList.contains("show"), false);
});

test("the dismiss control hides the notification and cancels its timer", (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const { toast, dismiss } = createToast();
  const ui = createUiController({ resultMsgEl: toast });

  ui.showMessage("Connection failed", true);
  dismiss();
  assert.equal(toast.classList.contains("show"), false);
  assert.equal(toast.inert, true);
  ui.showMessage("Try again", true);
  t.mock.timers.tick(2800);
  assert.equal(toast.classList.contains("show"), true);
});

test("missing notification elements are harmless", () => {
  assert.doesNotThrow(() => createUiController({}).showMessage("Unavailable", true));
});
