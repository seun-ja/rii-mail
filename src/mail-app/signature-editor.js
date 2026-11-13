import { plainTextToComposerHtml } from "./composer-fonts.js";
import {
  normalizeSignature,
  normalizeSignatureUrl,
  sanitizeSignatureHtml,
} from "./signature.js";

function escapeAttribute(value) {
  return String(value)
    .replaceAll("&", "&amp;")
    .replaceAll('"', "&quot;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;");
}

export function initializeSignatureEditor({
  editor,
  toolbar,
  fontSelect,
  documentRef = document,
  promptForValue = window.prompt.bind(window),
  onError = () => {},
}) {
  if (!editor || !toolbar) return null;

  let savedRange = null;

  const selectionIsInsideEditor = (selection) =>
    Boolean(
      selection?.rangeCount &&
        selection.anchorNode &&
        (selection.anchorNode === editor || editor.contains(selection.anchorNode)),
    );

  const saveSelection = () => {
    const selection = documentRef.getSelection?.();
    if (selectionIsInsideEditor(selection)) {
      savedRange = selection.getRangeAt(0).cloneRange();
    }
  };

  const restoreSelection = () => {
    const selection = documentRef.getSelection?.();
    if (!selection || !savedRange) {
      editor.focus();
      return selection;
    }

    selection.removeAllRanges();
    selection.addRange(savedRange);
    return selection;
  };

  const runCommand = (command, value = null) => {
    restoreSelection();
    documentRef.execCommand?.("styleWithCSS", false, true);
    documentRef.execCommand?.(command, false, value);
    editor.focus();
    saveSelection();
  };

  const getInsertionRange = () => {
    const selection = restoreSelection();
    if (selectionIsInsideEditor(selection)) return selection.getRangeAt(0);
    if (!selection || !documentRef.createRange) return null;

    const range = documentRef.createRange();
    range.selectNodeContents(editor);
    range.collapse(false);
    selection.removeAllRanges();
    selection.addRange(range);
    return range;
  };

  const insertHtml = (html) => {
    restoreSelection();
    if (documentRef.execCommand?.("insertHTML", false, html)) {
      saveSelection();
      return true;
    }

    const range = getInsertionRange();
    if (!range || !documentRef.createElement) return false;

    const template = documentRef.createElement("template");
    template.innerHTML = html;
    const fragment = template.content.cloneNode(true);
    const lastNode = fragment.lastChild;
    range.deleteContents();
    range.insertNode(fragment);

    if (lastNode) {
      const selection = documentRef.getSelection?.();
      range.setStartAfter(lastNode);
      range.collapse(true);
      selection?.removeAllRanges();
      selection?.addRange(range);
    }
    saveSelection();
    return true;
  };

  const insertLink = () => {
    const rawUrl = promptForValue("Enter a link URL", "https://");
    if (rawUrl === null) return;

    const url = normalizeSignatureUrl(rawUrl);
    if (!url) {
      onError("Enter a valid http, https, or mailto link.");
      return;
    }

    const range = getInsertionRange();
    if (range && !range.collapsed) {
      if (!documentRef.execCommand?.("createLink", false, url)) {
        const anchor = documentRef.createElement("a");
        anchor.setAttribute("href", url);
        anchor.setAttribute("target", "_blank");
        anchor.setAttribute("rel", "noopener noreferrer");
        anchor.appendChild(range.extractContents());
        range.insertNode(anchor);
      }
      saveSelection();
      return;
    }

    if (
      !insertHtml(
        `<a href="${escapeAttribute(url)}" target="_blank" rel="noopener noreferrer">${plainTextToComposerHtml(url)}</a>`,
      )
    ) {
      onError("Could not insert the link in this editor.");
    }
  };

  const insertImage = () => {
    const rawUrl = promptForValue("Enter a public image URL", "https://");
    if (rawUrl === null) return;

    const url = normalizeSignatureUrl(rawUrl, { image: true });
    if (!url) {
      onError("Enter a valid http or https image URL.");
      return;
    }

    if (
      !insertHtml(
        `<img src="${escapeAttribute(url)}" alt="" loading="lazy" style="max-width: 100%; height: auto">`,
      )
    ) {
      onError("Could not insert the image in this editor.");
    }
    editor.focus();
  };

  editor.addEventListener("keyup", saveSelection);
  editor.addEventListener("mouseup", saveSelection);
  editor.addEventListener("input", saveSelection);
  editor.addEventListener("click", (event) => {
    if (event.target.closest?.("a")) event.preventDefault();
  });
  editor.addEventListener("paste", (event) => {
    event.preventDefault();
    if (!event.clipboardData) {
      onError("Clipboard content could not be read safely.");
      return;
    }

    const html = event.clipboardData.getData("text/html");
    const text = event.clipboardData.getData("text/plain");
    const safeHtml = html
      ? sanitizeSignatureHtml(html, documentRef)
      : plainTextToComposerHtml(text);
    if (safeHtml) insertHtml(safeHtml);
  });
  editor.addEventListener("dragover", (event) => {
    event.preventDefault();
  });
  editor.addEventListener("drop", (event) => {
    event.preventDefault();
    const html = event.dataTransfer?.getData("text/html");
    const text = event.dataTransfer?.getData("text/plain");
    const safeHtml = html
      ? sanitizeSignatureHtml(html, documentRef)
      : plainTextToComposerHtml(text ?? "");
    if (safeHtml) insertHtml(safeHtml);
  });

  toolbar.addEventListener("mousedown", (event) => {
    if (event.target.closest?.("button")) event.preventDefault();
  });
  toolbar.addEventListener("click", (event) => {
    const command = event.target.closest?.("[data-signature-command]")?.dataset
      .signatureCommand;
    const action = event.target.closest?.("[data-signature-action]")?.dataset
      .signatureAction;

    if (command) runCommand(command);
    else if (action === "link") insertLink();
    else if (action === "image") insertImage();
  });
  fontSelect?.addEventListener("change", () => {
    runCommand("fontName", fontSelect.value);
  });

  return {
    getHtml() {
      return normalizeSignature(editor.innerHTML, documentRef);
    },
    setHtml(html) {
      editor.innerHTML = normalizeSignature(html, documentRef);
      savedRange = null;
    },
    setEnabled(enabled) {
      editor.contentEditable = String(Boolean(enabled));
      editor.setAttribute("aria-disabled", String(!enabled));
      toolbar.querySelectorAll("button, select").forEach((control) => {
        control.disabled = !enabled;
      });
    },
    focus() {
      editor.focus();
    },
  };
}
