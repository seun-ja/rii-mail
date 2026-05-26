export function getErrorMessage(error) {
  if (error instanceof Error) {
    return error.message;
  }

  if (typeof error === "string") {
    return error;
  }

  if (error && typeof error === "object") {
    return error.message || error.msg || JSON.stringify(error);
  }

  return "Unknown error";
}
