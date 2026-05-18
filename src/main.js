const { invoke } = window.__TAURI__.core;

window.addEventListener("DOMContentLoaded", () => {
  const subjectInputEl = document.querySelector("#subject-input");
  const fromInputEl = document.querySelector("#from-input");
  const bodyInputEl = document.querySelector("#body-input");
  const resultMsgEl = document.querySelector("#result-msg");

  document
    .querySelector("#email-form")
    .addEventListener("submit", async (e) => {
      e.preventDefault();

      const payload = {
        subject: subjectInputEl.value.trim(),
        emailFrom: fromInputEl.value.trim(),
        body: bodyInputEl.value.trim(),
      };

      try {
        const rating = await invoke("rater", {
          subject: payload.subject,
          emailFrom: payload.emailFrom,
          body: payload.body,
        });

        const score =
          typeof rating?.score === "number" ? rating.score.toFixed(2) : "N/A";
        const label = rating?.label ?? "Unknown";
        resultMsgEl.textContent = `Spam rating: ${label} (score: ${score})`;
      } catch (error) {
        resultMsgEl.textContent = `Failed to process email: ${error}`;
      }

      console.log("Captured email input", payload);
    });
});
