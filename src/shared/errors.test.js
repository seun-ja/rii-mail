import test from "node:test";
import assert from "node:assert/strict";

import { getErrorMessage } from "./errors.js";

test("getErrorMessage handles Error objects", () => {
  const msg = getErrorMessage(new Error("boom"));
  assert.equal(msg, "boom");
});

test("getErrorMessage handles strings", () => {
  assert.equal(getErrorMessage("plain error"), "plain error");
});

test("getErrorMessage handles object message and msg fields", () => {
  assert.equal(getErrorMessage({ message: "from message" }), "from message");
  assert.equal(getErrorMessage({ msg: "from msg" }), "from msg");
});

test("getErrorMessage falls back to JSON payload or unknown", () => {
  assert.equal(getErrorMessage({ code: 500 }), '{"code":500}');
  assert.equal(getErrorMessage(null), "Unknown error");
});
