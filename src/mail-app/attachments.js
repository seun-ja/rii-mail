export function getPreviewableAttachments(email) {
  if (!Array.isArray(email?.attachments)) {
    return [];
  }

  return email.attachments.filter(
    (attachment) =>
      attachment &&
      typeof attachment.partId === "string" &&
      /^\d+(?:\.\d+)*$/.test(attachment.partId) &&
      attachment.partId.split(".").every((part) => Number(part) > 0),
  );
}

export function decodeBase64Attachment(value) {
  const binary = globalThis.atob(String(value ?? ""));
  const bytes = new Uint8Array(binary.length);

  for (let index = 0; index < binary.length; index += 1) {
    bytes[index] = binary.charCodeAt(index);
  }

  return bytes;
}

export function createAttachmentPreview(
  response,
  { urlApi = URL, BlobCtor = Blob, TextDecoderCtor = TextDecoder } = {},
) {
  if (!response || typeof response.base64Data !== "string") {
    throw new Error("The attachment preview response is invalid.");
  }

  const contentType = String(response.contentType || "application/octet-stream");
  const bytes = decodeBase64Attachment(response.base64Data);
  const kind = contentType.startsWith("image/")
    ? "image"
    : contentType === "application/pdf"
      ? "pdf"
      : contentType.startsWith("text/") ||
          ["application/json", "application/xml"].includes(contentType)
        ? "text"
        : "unsupported";
  let objectUrl = null;
  let text = null;

  if (kind === "image" || kind === "pdf") {
    objectUrl = urlApi.createObjectURL(
      new BlobCtor([bytes], { type: contentType }),
    );
  } else if (kind === "text") {
    text = new TextDecoderCtor().decode(bytes);
  }

  return {
    kind,
    objectUrl,
    text,
    dispose() {
      if (objectUrl) {
        urlApi.revokeObjectURL(objectUrl);
        objectUrl = null;
      }
    },
  };
}

export function formatAttachmentSize(size) {
  const bytes = Number(size);
  if (!Number.isFinite(bytes) || bytes < 0) {
    return "";
  }

  if (bytes < 1024) {
    return `${bytes} B`;
  }

  const units = ["KB", "MB", "GB"];
  let value = bytes / 1024;
  let unitIndex = 0;

  while (value >= 1024 && unitIndex < units.length - 1) {
    value /= 1024;
    unitIndex += 1;
  }

  const precision = value >= 10 ? 0 : 1;
  return `${value.toFixed(precision)} ${units[unitIndex]}`;
}
