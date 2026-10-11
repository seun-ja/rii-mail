const SUPPORTED_PROVIDERS = new Set([
  "gmail",
  "yahoo",
  "outlook",
  "custom",
]);

export function getProviderLiteral(storage = window.localStorage) {
  const savedImapServer = (
    storage.getItem("riimail.imapServer") || ""
  ).toLowerCase();

  if (savedImapServer) {
    const provider = ["yahoo", "gmail", "outlook"].find((candidate) =>
      savedImapServer.includes(candidate),
    ) || "custom";

    storage.setItem("riimail.provider", provider);
    return provider;
  }

  const savedProvider = storage.getItem("riimail.provider")?.toLowerCase();
  if (SUPPORTED_PROVIDERS.has(savedProvider)) {
    return savedProvider;
  }

  // Keep frontend fallback aligned with backend provider inference.
  return "custom";
}
