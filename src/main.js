const { invoke } = window.__TAURI__.core;

window.addEventListener("DOMContentLoaded", () => {
  const subjectInputEl = document.querySelector("#subject-input");
  const fromInputEl = document.querySelector("#from-input");
  const bodyInputEl = document.querySelector("#body-input");
  const resultMsgEl = document.querySelector("#result-msg");
  const emailForm = document.querySelector("#email-form");

  async function checkInitStatus() {
    try {
      const status = await invoke("check_init_status");

      if (status === "setup") {
        window.location.replace("/setup.html");
        return false;
      } else if (status === "login") {
        window.location.replace("/login.html");
        return false;
      } else if (status === "signed_in") {
        return true;
      }

      return false;
    } catch (err) {
      resultMsgEl.textContent = `Failed to check initialization status`;
      return false;
    }
  }

  checkInitStatus();

  emailForm.addEventListener("submit", async (e) => {
    e.preventDefault();

    // Optionally re-check config before submit
    if (!(await checkInitStatus())) return;

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
