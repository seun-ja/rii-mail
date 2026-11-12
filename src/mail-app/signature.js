import { plainTextToComposerHtml } from "./composer-fonts.js";

export const SIGNATURE_MARKER = 'data-riimail-signature="true"';

const ALLOWED_TAGS = new Set([
  "a",
  "b",
  "br",
  "div",
  "em",
  "i",
  "img",
  "li",
  "ol",
  "p",
  "span",
  "strong",
  "u",
  "ul",
]);
const ALLOWED_STYLE_PROPERTIES = new Set([
  "color",
  "font-family",
  "font-size",
  "font-style",
  "font-weight",
  "text-align",
  "text-decoration",
]);

export function normalizeSignatureUrl(value, { image = false } = {}) {
  const rawUrl = String(value ?? "").trim();
  if (!rawUrl) return null;

  try {
    const url = new URL(rawUrl);
    const protocols = image ? ["http:", "https:"] : ["http:", "https:", "mailto:"];
    return protocols.includes(url.protocol) ? url.href : null;
  } catch {
    return null;
  }
}

function sanitizeStyle(style) {
  return String(style ?? "")
    .split(";")
    .map((declaration) => declaration.trim())
    .filter(Boolean)
    .map((declaration) => {
      const separator = declaration.indexOf(":");
      if (separator < 1) return null;

      const property = declaration.slice(0, separator).trim().toLowerCase();
      const value = declaration.slice(separator + 1).trim();
      if (
        !ALLOWED_STYLE_PROPERTIES.has(property) ||
        !value ||
        value.length > 160 ||
        /(?:expression|url|@import|[<>])/i.test(value)
      ) {
        return null;
      }

      return `${property}: ${value}`;
    })
    .filter(Boolean)
    .join("; ");
}

export function sanitizeSignatureHtml(html, documentRef = globalThis.document) {
  const value = String(html ?? "");
  if (!value.trim()) return "";

  if (!documentRef?.createElement || !documentRef?.createTextNode) {
    return plainTextToComposerHtml(value);
  }

  const template = documentRef.createElement("template");
  const output = documentRef.createElement("div");
  template.innerHTML = value;

  const appendSanitizedNode = (node, parent) => {
    if (node.nodeType === 3) {
      parent.appendChild(documentRef.createTextNode(node.textContent ?? ""));
      return;
    }
    if (node.nodeType !== 1) return;

    const tagName = node.tagName.toLowerCase();
    if (!ALLOWED_TAGS.has(tagName)) {
      Array.from(node.childNodes).forEach((child) =>
        appendSanitizedNode(child, parent),
      );
      return;
    }

    if (tagName === "img") {
      const source = normalizeSignatureUrl(node.getAttribute("src"), { image: true });
      if (!source) return;

      const image = documentRef.createElement("img");
      image.setAttribute("src", source);
      image.setAttribute("alt", String(node.getAttribute("alt") ?? ""));
      image.setAttribute("loading", "lazy");
      image.setAttribute("style", "max-width: 100%; height: auto");
      parent.appendChild(image);
      return;
    }

    const element = documentRef.createElement(tagName);
    const style = sanitizeStyle(node.getAttribute("style"));
    if (style) element.setAttribute("style", style);

    if (tagName === "a") {
      const href = normalizeSignatureUrl(node.getAttribute("href"));
      if (href) {
        element.setAttribute("href", href);
        element.setAttribute("target", "_blank");
        element.setAttribute("rel", "noopener noreferrer");
      }
    }

    Array.from(node.childNodes).forEach((child) =>
      appendSanitizedNode(child, element),
    );
    parent.appendChild(element);
  };

  Array.from(template.content.childNodes).forEach((node) =>
    appendSanitizedNode(node, output),
  );
  return output.innerHTML;
}

function signatureHasContent(html, documentRef = globalThis.document) {
  const value = String(html ?? "");
  if (!value.trim()) return false;

  if (!documentRef?.createElement) {
    return /<img\b/i.test(value) || value.replace(/<[^>]*>/g, "").trim().length > 0;
  }

  const template = documentRef.createElement("template");
  template.innerHTML = value;
  return Boolean(
    template.content.textContent?.trim() || template.content.querySelector("img"),
  );
}

export function normalizeSignature(signature, documentRef = globalThis.document) {
  const sanitized = sanitizeSignatureHtml(signature, documentRef);
  return signatureHasContent(sanitized, documentRef) ? sanitized : "";
}

export function getComposerSignatureHtml(editor, documentRef = globalThis.document) {
  const signature = editor?.querySelector?.('[data-riimail-signature="true"]');
  return signature ? normalizeSignature(signature.innerHTML, documentRef) : null;
}

export function getAuthoredComposerText(editor) {
  const clone = editor?.cloneNode?.(true);
  if (!clone) return "";

  clone.querySelector?.('[data-riimail-signature="true"]')?.remove();
  return String(clone.innerText ?? clone.textContent ?? "").trim();
}

export function containsSignature(html) {
  return String(html ?? "").includes(SIGNATURE_MARKER);
}

export function createSignatureHtml(signature) {
  const value = String(signature ?? "").trim();
  if (!value) return "";

  return `<div ${SIGNATURE_MARKER}>${value}</div>`;
}

function plainTextSection(text) {
  const value = String(text ?? "").trim();
  return value ? plainTextToComposerHtml(value) : "";
}

function joinSections(sections) {
  return sections.filter(Boolean).join("<br><br>");
}

export function createSignedDraftHtml({
  body = "",
  bodyHtml = "",
  signature = "",
  signatureBeforeBody = false,
} = {}) {
  const existingHtml = String(bodyHtml ?? "");
  if (existingHtml && containsSignature(existingHtml)) return existingHtml;

  const originalContent = existingHtml || plainTextToComposerHtml(body);
  const signatureHtml = createSignatureHtml(signature);
  if (!signatureHtml) return originalContent;

  const content = existingHtml || plainTextSection(body);
  if (signatureBeforeBody) {
    return `<div><br></div>${signatureHtml}${content ? `<br><br>${content}` : ""}`;
  }

  return content
    ? joinSections([content, signatureHtml])
    : `<div><br></div>${signatureHtml}`;
}

export function createGeneratedDraftHtml({
  body = "",
  signature = "",
  trailingContent = "",
} = {}) {
  const messageHtml = plainTextSection(body);
  const signatureHtml = createSignatureHtml(signature);
  const trailingHtml = plainTextSection(trailingContent);

  return joinSections([messageHtml, signatureHtml, trailingHtml]);
}
