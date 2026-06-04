export function getProviderLiteral(storage = window.localStorage) {
  const savedImapServer = (
    storage.getItem("riimail.imapServer") || ""
  ).toLowerCase();

  if (savedImapServer.includes("yahoo")) {
    storage.setItem("riimail.provider", "yahoo");
    return "yahoo";
  }

  if (savedImapServer.includes("gmail")) {
    storage.setItem("riimail.provider", "gmail");
    return "gmail";
  }

  const savedProvider = storage.getItem("riimail.provider");
  if (savedProvider === "gmail" || savedProvider === "yahoo") {
    return savedProvider;
  }

  // Keep frontend fallback aligned with backend provider inference.
  return "gmail";
}
