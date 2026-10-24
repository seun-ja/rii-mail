export async function updateAiAssistantAvailability({
  invoke,
  button,
  panel,
}) {
  button?.classList.add("hidden");
  panel?.classList.add("hidden");

  if (!invoke || !button) {
    return false;
  }

  try {
    const enabled = (await invoke("is_llm_enabled")) === true;
    button.classList.toggle("hidden", !enabled);
    return enabled;
  } catch (_error) {
    // Keep AI controls unavailable when provider state cannot be verified.
    return false;
  }
}
