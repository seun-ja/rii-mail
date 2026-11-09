export const DEFAULT_COMPOSER_FONT = "aptos";

const COMPOSER_FONTS = Object.freeze([
  {
    value: "aptos",
    editorFamily: '"Aptos", Arial, sans-serif',
    commandFamily: "Aptos",
  },
  {
    value: "arial",
    editorFamily: "Arial, sans-serif",
    commandFamily: "Arial",
  },
  {
    value: "calibri",
    editorFamily: "Calibri, Arial, sans-serif",
    commandFamily: "Calibri",
  },
  {
    value: "georgia",
    editorFamily: "Georgia, serif",
    commandFamily: "Georgia",
  },
  {
    value: "times-new-roman",
    editorFamily: '"Times New Roman", Times, serif',
    commandFamily: "Times New Roman",
  },
  {
    value: "verdana",
    editorFamily: "Verdana, sans-serif",
    commandFamily: "Verdana",
  },
  {
    value: "courier-new",
    editorFamily: '"Courier New", monospace',
    commandFamily: "Courier New",
  },
]);

const TOGGLE_COMMANDS = new Set(["bold", "italic", "underline"]);

export function getComposerFont(value) {
  return (
    COMPOSER_FONTS.find((font) => font.value === value) ?? COMPOSER_FONTS[0]
  );
}

function applyComposerFont(select, editor, value = select?.value) {
  const font = getComposerFont(value);
  if (select) select.value = font.value;
  if (editor) editor.style.fontFamily = font.editorFamily;
}

export function plainTextToComposerHtml(text) {
  return String(text ?? "")
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replace(/\r\n?/g, "\n")
    .replaceAll("\n", "<br>");
}

export function getComposerText(editor) {
  return String(editor?.innerText ?? editor?.textContent ?? "");
}

export function getComposerHtml(editor) {
  return String(editor?.innerHTML ?? "");
}

export function setComposerText(editor, text) {
  if (editor) editor.innerHTML = plainTextToComposerHtml(text);
}

export function setComposerHtml(editor, html) {
  if (editor) editor.innerHTML = String(html ?? "");
}

export function createComposerHtmlBody(editorHtml) {
  return `<div style="font-family: Aptos, Arial, sans-serif; font-size: 11pt; line-height: 1.5;">${String(editorHtml ?? "")}</div>`;
}

function selectionIsInsideEditor(selection, editor) {
  return Boolean(
    selection?.rangeCount &&
      selection.anchorNode &&
      (selection.anchorNode === editor || editor.contains(selection.anchorNode)),
  );
}

export function initializeComposerEditor({
  editor,
  toolbar,
  fontSelect,
  sizeSelect,
  documentRef = document,
}) {
  if (!editor || !toolbar) return null;

  applyComposerFont(fontSelect, editor, DEFAULT_COMPOSER_FONT);
  let savedRange = null;

  const saveSelection = () => {
    const selection = documentRef.getSelection?.();
    if (selectionIsInsideEditor(selection, editor)) {
      savedRange = selection.getRangeAt(0).cloneRange();
    }
  };

  const restoreSelection = () => {
    const selection = documentRef.getSelection?.();
    if (!selection || !savedRange) {
      editor.focus();
      return;
    }
    selection.removeAllRanges();
    selection.addRange(savedRange);
  };

  const updateButtonStates = () => {
    toolbar.querySelectorAll("[data-format-command]").forEach((button) => {
      const command = button.dataset.formatCommand;
      if (!TOGGLE_COMMANDS.has(command)) return;
      let isActive = false;
      try {
        isActive = Boolean(documentRef.queryCommandState?.(command));
      } catch (_) {
        isActive = false;
      }
      button.classList.toggle("active", isActive);
      button.setAttribute("aria-pressed", String(isActive));
    });
  };

  const runCommand = (command, value = null) => {
    restoreSelection();
    documentRef.execCommand?.("styleWithCSS", false, true);
    documentRef.execCommand?.(command, false, value);
    editor.focus();
    saveSelection();
    updateButtonStates();
  };

  documentRef.addEventListener?.("selectionchange", () => {
    saveSelection();
    updateButtonStates();
  });
  editor.addEventListener("keyup", saveSelection);
  editor.addEventListener("mouseup", saveSelection);
  editor.addEventListener("input", saveSelection);

  toolbar.addEventListener("mousedown", (event) => {
    if (event.target.closest?.("[data-format-command]")) {
      event.preventDefault();
    }
  });
  toolbar.addEventListener("click", (event) => {
    const button = event.target.closest?.("[data-format-command]");
    if (button) runCommand(button.dataset.formatCommand);
  });

  fontSelect?.addEventListener("change", () => {
    const font = getComposerFont(fontSelect.value);
    runCommand("fontName", font.commandFamily);
  });
  sizeSelect?.addEventListener("change", () => {
    runCommand("fontSize", sizeSelect.value);
  });

  editor.addEventListener("paste", (event) => {
    const text = event.clipboardData?.getData("text/plain");
    if (typeof text !== "string") return;
    event.preventDefault();
    documentRef.execCommand?.("insertText", false, text);
  });

  return {
    reset() {
      setComposerText(editor, "");
      applyComposerFont(fontSelect, editor, DEFAULT_COMPOSER_FONT);
      if (sizeSelect) sizeSelect.value = "3";
      savedRange = null;
      updateButtonStates();
    },
  };
}
