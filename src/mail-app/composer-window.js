import { getMailAppDom } from "./dom.js";
import { createUiController } from "./ui.js";
import { insertResponseBeforeQuote } from "./helpers.js";
import { updateAiAssistantAvailability } from "./ai-assistant.js";
import { getErrorMessage } from "../shared/errors.js";

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
  dom.composeBodyInput.value = draft.body || "";

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
        dom.composeBodyInput.value = quotedEmailContext
          ? insertResponseBeforeQuote(
              email.body,
              quotedEmailContext,
              dom.composeBodyInput.value,
            )
          : (email.body ?? dom.composeBodyInput.value);

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
    const body = dom.composeBodyInput.value.trim();

    if (toEmails.length === 0) {
      ui.showMessage("Recipient email is required.", true);
      return;
    }
    if (!subject || !body) {
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
        body,
        contentType: "text/plain",
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
}
