const { invoke } = window.__TAURI__.core;

const DEFAULT_CONFIG = {
  rpc_server: import.meta?.env?.VITE_RPC_SERVER || "0.0.0.0:5500",
  rust_log: import.meta?.env?.VITE_RUST_LOG || "info",
  otlp_collector_endpoint:
    import.meta?.env?.VITE_OTLP_COLLECTOR_ENDPOINT ||
    "http://otel-collector:4317",
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

    const imapServerInput = document.querySelector("#imap-server-input");
    const imapPortInput = document.querySelector("#imap-port-input");

    const imapServer = imapServerInput?.value?.trim() || "";
    const imapPortValue = imapPortInput?.value?.trim() || "";

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
      try {
        // Flatten config object for Tauri IPC (Tauri converts Rust snake_case to camelCase)
        await invoke("config_setup", {
          rpcServer: config.rpc_server,
          imapServer: config.imap_server,
          imapPort: config.imap_port,
          rustLog: config.rust_log,
          otlpCollectorEndpoint: config.otlp_collector_endpoint,
        });
      } catch (setupError) {
        const setupErrorMsg = setupError?.message || JSON.stringify(setupError);
        throw setupError;
      }
      setupMsgEl.textContent = "Setup complete. Waiting for initialization...";

      // Poll is_initialized up to 10 times, 300ms apart
      let initialized = false;
      for (let i = 0; i < 10; i++) {
        try {
          initialized = await invoke("is_initialized");
          if (initialized) break;
        } catch (pollError) {
          const pollErrorMsg =
            pollError instanceof Error
              ? pollError.message
              : JSON.stringify(pollError);
        }
        await new Promise((res) => setTimeout(res, 300));
      }
      if (initialized) {
        window.location.reload();
      } else {
        setupMsgEl.textContent =
          "Setup saved, but backend did not initialize. Check console for errors and try reloading the app.";
      }
    } catch (error) {
      let errorMessage = "Unknown error";
      if (error instanceof Error) {
        errorMessage = error.message;
      } else if (typeof error === "string") {
        errorMessage = error;
      } else if (error && typeof error === "object") {
        // Handle Rust Error struct serialized as object with 'message' field
        errorMessage = error.message || error.msg || JSON.stringify(error);
      }
      setupMsgEl.textContent = `Setup failed: ${errorMessage}`;
    }
  });
});
