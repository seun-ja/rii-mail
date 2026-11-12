export function activateSettingsSection(
  { navButtons = [], panels = [] } = {},
  requestedSection,
) {
  const buttons = Array.from(navButtons);
  const availableSections = new Set(
    buttons.map((button) => button.dataset.settingsSection).filter(Boolean),
  );
  const section = availableSections.has(requestedSection)
    ? requestedSection
    : buttons[0]?.dataset.settingsSection;

  if (!section) return null;

  buttons.forEach((button) => {
    const isActive = button.dataset.settingsSection === section;
    button.classList.toggle("active", isActive);
    button.setAttribute("aria-selected", String(isActive));
  });

  Array.from(panels).forEach((panel) => {
    panel.classList.toggle(
      "hidden",
      panel.dataset.settingsPanel !== section,
    );
  });

  return section;
}
