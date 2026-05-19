const { invoke } = window.__TAURI__.core;

const DEFAULT_CONFIG = {
  rpc_server: import.meta.env.VITE_RPC_SERVER || "0.0.0.0:5500",
  rust_log: import.meta.env.VITE_RUST_LOG || "info",
  otlp_collector_endpoint:
    import.meta.env.VITE_OTLP_COLLECTOR_ENDPOINT || "http://otel-collector:4317",
};

window.addEventListener("DOMContentLoaded", () => {
  const setupForm = document.querySelector("#setup-form");
  const setupMsgEl = document.querySelector("#setup-msg");

  async function checkAlreadyInitialized() {
    try {
      const initialized = await invoke("is_initialized");
      if (initialized) {
        window.location.replace("/");
      }
    } catch (error) {
      setupMsgEl.textContent = `Could not check initialization status: ${error}`;
    }
  }

  checkAlreadyInitialized();

  setupForm.addEventListener("submit", async (event) => {
    event.preventDefault();

    const imapServer = document.querySelector("#imap-server-input").value.trim();
    const imapPortValue = document.querySelector("#imap-port-input").value.trim();

    const config = {
      rpc_server: DEFAULT_CONFIG.rpc_server,
      imap_server: imapServer,
      imap_port: Number.parseInt(imapPortValue, 10),
      rust_log: DEFAULT_CONFIG.rust_log,
      otlp_collector_endpoint: DEFAULT_CONFIG.otlp_collector_endpoint,
    };

    if (Number.isNaN(config.imap_port)) {
      setupMsgEl.textContent = "IMAP port must be a valid number.";
      return;
    }

    try {
      setupMsgEl.textContent = "Saving configuration...";
      await invoke("config_setup", { config });
      setupMsgEl.textContent = "Setup complete. Waiting for initialization...";

      // Poll is_initialized up to 10 times, 300ms apart
      let initialized = false;
      for (let i = 0; i < 10; i++) {
        try {
          initialized = await invoke("is_initialized");
          if (initialized) break;
        } catch {}
        await new Promise((res) => setTimeout(res, 300));
      }
      if (initialized) {
        window.location.reload();
      } else {
        setupMsgEl.textContent = "Setup saved, but backend did not initialize in time. Please reload the app.";
      }
    } catch (error) {
      setupMsgEl.textContent = `Setup failed: ${error}`;
    }
  });
});
