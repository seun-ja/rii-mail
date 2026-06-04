import test from "node:test";
import assert from "node:assert/strict";

import { getProviderLiteral } from "./provider.js";

function createStorage(initial = {}) {
  const store = new Map(Object.entries(initial));
  return {
    getItem(key) {
      return store.has(key) ? store.get(key) : null;
    },
    setItem(key, value) {
      store.set(key, String(value));
    },
  };
}

test("getProviderLiteral infers yahoo from imap server and persists", () => {
  const storage = createStorage({
    "riimail.imapServer": "imap.mail.yahoo.com",
  });

  const provider = getProviderLiteral(storage);

  assert.equal(provider, "yahoo");
  assert.equal(storage.getItem("riimail.provider"), "yahoo");
});

test("getProviderLiteral infers gmail from imap server and persists", () => {
  const storage = createStorage({ "riimail.imapServer": "imap.gmail.com" });

  const provider = getProviderLiteral(storage);

  assert.equal(provider, "gmail");
  assert.equal(storage.getItem("riimail.provider"), "gmail");
});

test("getProviderLiteral returns saved valid provider when imap server is absent", () => {
  const gmailStorage = createStorage({ "riimail.provider": "gmail" });
  assert.equal(getProviderLiteral(gmailStorage), "gmail");

  const yahooStorage = createStorage({ "riimail.provider": "yahoo" });
  assert.equal(getProviderLiteral(yahooStorage), "yahoo");
});

test("getProviderLiteral falls back to gmail for unknown values", () => {
  const storage = createStorage({ "riimail.provider": "other" });
  assert.equal(getProviderLiteral(storage), "gmail");
});
