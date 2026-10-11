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

test("getProviderLiteral infers outlook from imap server and persists", () => {
  const storage = createStorage({
    "riimail.imapServer": "imap-mail.outlook.com",
  });

  const provider = getProviderLiteral(storage);

  assert.equal(provider, "outlook");
  assert.equal(storage.getItem("riimail.provider"), "outlook");
});

test("getProviderLiteral treats an unknown IMAP server as custom", () => {
  const storage = createStorage({
    "riimail.imapServer": "mail.example.com",
    "riimail.provider": "gmail",
  });

  const provider = getProviderLiteral(storage);

  assert.equal(provider, "custom");
  assert.equal(storage.getItem("riimail.provider"), "custom");
});

test("getProviderLiteral returns any saved supported provider when imap server is absent", () => {
  for (const provider of ["gmail", "yahoo", "outlook", "custom"]) {
    const storage = createStorage({ "riimail.provider": provider });
    assert.equal(getProviderLiteral(storage), provider);
  }
});

test("getProviderLiteral falls back to custom for unknown values", () => {
  const storage = createStorage({ "riimail.provider": "other" });
  assert.equal(getProviderLiteral(storage), "custom");
});
