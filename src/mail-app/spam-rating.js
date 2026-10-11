export function formatSpamRatingResponse(rating) {
  const rawScore = rating?.score;
  const hasScore = typeof rawScore === "number";

  const label = rating?.label ?? "Unknown";
  const labelText = label === "Ham" ? "safe" : label.toLowerCase();

  if (!hasScore) {
    return "Could not determine email status (score: N/A)";
  }

  const displayScore = rawScore.toFixed(2);

  if (rawScore <= 0.2) {
    return `This email is likely ${labelText} (score: ${displayScore})`;
  }

  if (rawScore >= 0.8) {
    return `This email is ${labelText} (score: ${displayScore})`;
  }

  return `I think it's ${labelText} (score: ${displayScore})`;
}

export function formatSpamRatingError(error) {
  return `Failed to check spam rating: ${error}`;
}
