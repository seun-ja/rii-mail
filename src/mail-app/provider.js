export function getProviderLiteral(storage = window.localStorage) {
  const savedImapServer = (storage.getItem("pemail.imapServer") || "").toLowerCase();

  if (savedImapServer.includes("yahoo")) {
    storage.setItem("pemail.provider", "yahoo");
    return "yahoo";
  }

  if (savedImapServer.includes("gmail")) {
    storage.setItem("pemail.provider", "gmail");
    return "gmail";
  }

  const savedProvider = storage.getItem("pemail.provider");
  if (savedProvider === "gmail" || savedProvider === "yahoo") {
    return savedProvider;
  }

  return "yahoo";
}
