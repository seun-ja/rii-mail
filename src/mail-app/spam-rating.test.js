import test from "node:test";
import assert from "node:assert/strict";

import {
  formatSpamRatingError,
  formatSpamRatingResponse,
} from "./spam-rating.js";

test("formats Ham at the safe low boundary", () => {
  assert.equal(
    formatSpamRatingResponse({ label: "Ham", score: 0.2 }),
    "This email is likely safe (score: 0.20)",
  );
});

test("formats spam at the high boundary", () => {
  assert.equal(
    formatSpamRatingResponse({ label: "Spam", score: 0.8 }),
    "This email is spam (score: 0.80)",
  );
});

test("formats a middle score with the uncertain message", () => {
  assert.equal(
    formatSpamRatingResponse({ label: "Spam", score: 0.5 }),
    "I think it's spam (score: 0.50)",
  );
});

test("reports N/A for missing and non-number scores", async (t) => {
  const expected = "Could not determine email status (score: N/A)";
  const cases = [
    ["missing rating", undefined],
    ["missing score", { label: "Spam" }],
    ["null score", { label: "Spam", score: null }],
    ["numeric string score", { label: "Spam", score: "0.90" }],
  ];

  for (const [name, rating] of cases) {
    await t.test(name, () => {
      assert.equal(formatSpamRatingResponse(rating), expected);
    });
  }
});

test("normalizes unknown and missing labels", () => {
  assert.equal(
    formatSpamRatingResponse({ label: "Unknown", score: 0.1 }),
    "This email is likely unknown (score: 0.10)",
  );
  assert.equal(
    formatSpamRatingResponse({ score: 0.4 }),
    "I think it's unknown (score: 0.40)",
  );
});

test("formats scores to exactly two decimal places", () => {
  assert.equal(
    formatSpamRatingResponse({ label: "Ham", score: 0 }),
    "This email is likely safe (score: 0.00)",
  );
  assert.equal(
    formatSpamRatingResponse({ label: "Spam", score: 0.234 }),
    "I think it's spam (score: 0.23)",
  );
});

test("uses the raw score for boundaries before formatting it", () => {
  assert.equal(
    formatSpamRatingResponse({ label: "Spam", score: 0.799 }),
    "I think it's spam (score: 0.80)",
  );
  assert.equal(
    formatSpamRatingResponse({ label: "Spam", score: 0.801 }),
    "This email is spam (score: 0.80)",
  );
});

test("formats backend errors without changing their string representation", () => {
  assert.equal(
    formatSpamRatingError(new Error("backend unavailable")),
    "Failed to check spam rating: Error: backend unavailable",
  );
  assert.equal(
    formatSpamRatingError("request timed out"),
    "Failed to check spam rating: request timed out",
  );
});
