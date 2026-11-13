import { getMailAppDom } from "./dom.js";
import { createUiController } from "./ui.js";
import { updateAiAssistantAvailability } from "./ai-assistant.js";
import { getErrorMessage } from "../shared/errors.js";
import {
  createComposerHtmlBody,
  getComposerHtml,
  initializeComposerEditor,
  setComposerHtml,
} from "./composer-fonts.js";
import {
  createGeneratedDraftHtml,
  createSignedDraftHtml,
  getAuthoredComposerText,
  getComposerSignatureHtml,
  normalizeSignature,
} from "./signature.js";

const DETACHED_COMPOSER_KEY_PREFIX = "riimail.detachedComposer.";

export function readDetachedComposerDraft(storage, hash) {
  let storageKey;
  try {
    storageKey = decodeURIComponent(String(hash || "").replace(/^#/, ""));
  } catch (_) {
    return null;
  }
  if (!storageKey.startsWith(DETACHED_COMPOSER_KEY_PREFIX)) {
    return null;
  }

  const rawDraft = storage.getItem(storageKey);
  storage.removeItem(storageKey);
  if (!rawDraft) return null;

  try {
    const draft = JSON.parse(rawDraft);
    return draft && typeof draft === "object" ? draft : null;
  } catch (_) {
    return null;
  }
}

export function createDetachedComposerHtml(draft = {}) {
  const bodyHtml = String(draft.bodyHtml ?? "");
  if (bodyHtml) return bodyHtml;

  return createSignedDraftHtml({
    body: draft.body,
    signature: normalizeSignature(draft.signature),
    signatureBeforeBody: draft.title === "Reply" || draft.title === "Forward",
  });
}

function splitAddresses(value) {
  return String(value || "")
    .split(/[,;]/)
    .map((address) => address.trim())
    .filter(Boolean);
}

export function initializeComposerWindow({
  invoke,
  storage = window.localStorage,
  closeWindow = () => window.close(),
  emitToMain,
  notifyReady,
  listen,
}) {
  const draft = readDetachedComposerDraft(storage, window.location.hash) || {};
  const dom = getMailAppDom();
  const ui = createUiController(dom);

  document.body.classList.add("composer-window-page");
  dom.composeModalEl?.classList.remove("hidden");
  document.getElementById("compose-title").textContent =
    draft.title || "New Message";
  dom.composeToInput.value = draft.to || "";
  dom.composeCcInput.value = draft.cc || "";
  dom.composeBccInput.value = draft.bcc || "";
  dom.composeSubjectInput.value = draft.subject || "";
  const composerEditor = initializeComposerEditor({
    editor: dom.composeBodyInput,
    toolbar: dom.composeToolbar,
    fontSelect: dom.composeFontSelect,
    sizeSelect: dom.composeFontSizeSelect,
  });
  const signature = normalizeSignature(draft.signature);
  setComposerHtml(dom.composeBodyInput, createDetachedComposerHtml(draft));

  const ccRow = document.getElementById("compose-cc-row");
  const bccRow = document.getElementById("compose-bcc-row");
  const toggleCcBtn = document.getElementById("toggle-cc-btn");
  const toggleBccBtn = document.getElementById("toggle-bcc-btn");
  const aiPanel = dom.aiAssistantPanelEl;
  const aiContextSection = document.getElementById("ai-context-section");
  const aiContextCheckbox = document.getElementById(
    "ai-use-selected-email-context",
  );

  ccRow?.classList.toggle("hidden", !String(draft.cc || "").trim());
  bccRow?.classList.toggle("hidden", !String(draft.bcc || "").trim());
  toggleCcBtn?.setAttribute(
    "aria-expanded",
    String(Boolean(String(draft.cc || "").trim())),
  );
  toggleBccBtn?.setAttribute(
    "aria-expanded",
    String(Boolean(String(draft.bcc || "").trim())),
  );
  aiContextSection?.classList.toggle("hidden", !draft.isReply);

  if (typeof invoke !== "function") {
    ui.showMessage("Backend invoke bridge is unavailable.", true);
    return;
  }

  if (typeof listen === "function") {
    void listen("app://logged-out", () => {
      composerEditor?.reset();
      void closeWindow();
    }).catch(() => {
      // Ignore listener setup failures outside the Tauri runtime.
    });
  }

  void updateAiAssistantAvailability({
    invoke,
    button: dom.openAiAssistantBtnEl,
    panel: dom.aiAssistantPanelEl,
  });

  dom.closeComposeBtnEl?.addEventListener("click", () => {
    void closeWindow();
  });

  document.addEventListener("keydown", (event) => {
    if (event.key === "Escape" && aiPanel?.classList.contains("hidden")) {
      void closeWindow();
    }
  });

  toggleCcBtn?.addEventListener("click", () => {
    ccRow?.classList.toggle("hidden");
    const isExpanded = !ccRow?.classList.contains("hidden");
    toggleCcBtn.setAttribute("aria-expanded", String(isExpanded));
    if (isExpanded) dom.composeCcInput.focus();
  });

  toggleBccBtn?.addEventListener("click", () => {
    bccRow?.classList.toggle("hidden");
    const isExpanded = !bccRow?.classList.contains("hidden");
    toggleBccBtn.setAttribute("aria-expanded", String(isExpanded));
    if (isExpanded) dom.composeBccInput.focus();
  });

  dom.openAiAssistantBtnEl?.addEventListener("click", () => {
    aiPanel?.classList.remove("hidden");
  });

  document
    .getElementById("close-ai-assistant-btn")
    ?.addEventListener("click", () => {
      aiPanel?.classList.add("hidden");
    });

  document
    .getElementById("generate-email-btn")
    ?.addEventListener("click", async () => {
      const prompt = document.getElementById("ai-email-prompt").value.trim();
      if (!prompt) return;

      const status = document.getElementById("generate-email-status");
      const useReplyContext = Boolean(
        draft.isReply && aiContextCheckbox?.checked && draft.replyContext,
      );
      const quotedEmailContext = useReplyContext ? draft.replyContext : null;

      try {
        status.textContent = "Generating draft...";
        const email = await invoke("email_generator", {
          thoughts: prompt,
          context: quotedEmailContext,
        });

        dom.composeToInput.value = email.to ?? dom.composeToInput.value;
        dom.composeCcInput.value = email.cc ?? dom.composeCcInput.value;
        dom.composeBccInput.value = email.bcc ?? dom.composeBccInput.value;
        dom.composeSubjectInput.value =
          email.subject ?? dom.composeSubjectInput.value;
        const currentSignature = getComposerSignatureHtml(dom.composeBodyInput);
        composerEditor?.reset();
        setComposerHtml(
          dom.composeBodyInput,
          createGeneratedDraftHtml({
            body: email.body,
            signature: currentSignature ?? "",
            trailingContent: quotedEmailContext,
          }),
        );

        ccRow?.classList.toggle("hidden", !dom.composeCcInput.value.trim());
        bccRow?.classList.toggle("hidden", !dom.composeBccInput.value.trim());
        status.textContent = "Draft inserted";
      } catch (error) {
        console.error(error);
        status.textContent = "Could not generate a draft";
      }
    });

  dom.sendComposeBtnEl?.addEventListener("click", async () => {
    const toEmails = splitAddresses(dom.composeToInput.value);
    const ccEmails = splitAddresses(dom.composeCcInput.value);
    const bccEmails = splitAddresses(dom.composeBccInput.value);
    const subject = dom.composeSubjectInput.value.trim();
    const bodyText = getAuthoredComposerText(dom.composeBodyInput);

    if (toEmails.length === 0) {
      ui.showMessage("Recipient email is required.", true);
      return;
    }
    if (!subject || !bodyText) {
      ui.showMessage("Subject and body are required.", true);
      return;
    }

    try {
      dom.sendComposeBtnEl.disabled = true;
      const sentEmail = await invoke("send_email", {
        to: toEmails.map((email) => ["", email]),
        cc: ccEmails.map((email) => ["", email]),
        bcc: bccEmails.map((email) => ["", email]),
        subject,
        body: createComposerHtmlBody(getComposerHtml(dom.composeBodyInput)),
        contentType: "text/html; charset=utf-8",
      });

      if (sentEmail && typeof emitToMain === "function") {
        await emitToMain("composer://sent", sentEmail);
      }
      await closeWindow();
    } catch (error) {
      console.error(error);
      dom.sendComposeBtnEl.disabled = false;
      ui.showMessage(`Error sending email: ${getErrorMessage(error)}`, true);
    }
  });

  const inputToFocus = draft.to ? dom.composeBodyInput : dom.composeToInput;
  inputToFocus?.focus();

  if (typeof notifyReady === "function") {
    setTimeout(() => {
      void notifyReady();
    }, 0);
  }
}
