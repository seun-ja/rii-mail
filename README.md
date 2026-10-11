# RiiMail (Email Desktop Client)

RiiMail is a Tauri 2 desktop app for triaging email with IMAP sync and AI-assisted phishing/spam checks.

## Quick Start

To get started, you will need to configure your local environment:

1. Clone this repository.
2. Create a `.env` file in the root (see [Environment Variables](#environment-variables) below).
3. Install the [local inference requirements](#local-inference-requirements).
4. Run the application using `npm run dev`.

## What It Does

- Connects to an IMAP account (for example Gmail or Yahoo)
- Syncs and caches mailbox data locally (Inbox/Sent)
- Displays a desktop inbox experience
- Runs spam/phishing rating locally through PyO3, PyTorch, and Transformers
- Persists config and login state between launches
- Shows a startup loading state on auth pages while initialization status is being resolved

## Tech Stack

- Frontend: Vanilla JavaScript, HTML, CSS (served by Tauri webview)
- Backend: Rust + Tauri 2
- Mail: `async-imap` + TLS
- Storage: SQLite via `sqlx`
- AI calls: tarpc client (`rpc-agent`)

## Repository Layout

```text
src/
   index.html                # Main mailbox UI
   login.html                # Login screen
   setup.html                # First-run IMAP setup screen
   main.js                   # Frontend entry point (mail app init)
   login.js                  # Login flow (username/password)
   setup.js                  # Setup flow (IMAP server + port)
   mail-app/                 # Mail UI state, rendering, syncing, provider logic
   shared/                   # Shared frontend helpers

src-tauri/
   Cargo.toml                # Rust dependencies and crate metadata
   tauri.conf.json           # Tauri app/window/bundle config
   src/
      lib.rs                  # Tauri app bootstrap + command registration
      config.rs               # App config and init status types
      handlers/               # Tauri commands (setup, login, rater, email fetch)
      workers/                # Background IMAP worker loop
      db/                     # SQLite initialization and queries
      llm/                    # LLM provider and local-inference integration
```

## Prerequisites

- Rust toolchain (stable) via rustup
- Tauri CLI:

```bash
cargo install tauri-cli
```

On macOS, ensure Xcode Command Line Tools are installed.

### Local inference requirements

Spam rating runs locally through PyO3 and requires:

- A Python interpreter compatible with the project's PyO3 version. Python 3.12 is recommended for the current PyO3 0.21 dependency.
- Working `torch` and `transformers` installations in that interpreter.
- The classification model files in `merged-model-new/` at the repository root.

Create and activate a virtual environment before building or running Tauri so PyO3 links against the intended interpreter:

```bash
python3.12 -m venv .venv
source .venv/bin/activate
python -m pip install torch transformers
```

Verify the environment before starting the application:

```bash
python -c "import sys, torch, transformers; print(sys.version); print(torch.__version__); print(transformers.__version__)"
```

At runtime, RiiMail dynamically locates a Python interpreter whose major/minor version matches the interpreter embedded by PyO3 and verifies that `import torch` succeeds. It checks the active virtual environment, project virtual environments, environments under `~/.venvs`, and Python executables available on `PATH`.

## Environment Variables

Copy the example environment file in the repository root:

```bash
cp .env.example .env
```

The example contains:

```bash
RPC_SERVER=0.0.0.0:5500
RUST_LOG=info
TELEMETRY_OPT_IN=false
OTLP_COLLECTOR_ENDPOINT=http://0.0.0.0:4317
```

Notes:

- `RPC_SERVER` is retained for legacy RPC configuration; spam rating now uses local inference.
- `RUST_LOG` can be raised to `debug` during local troubleshooting.
- `TELEMETRY_OPT_IN` defaults to `false`. Set it to `true` to export traces to the configured OTLP collector.
- `OTLP_COLLECTOR_ENDPOINT` is only used when `TELEMETRY_OPT_IN=true`.

## Run Locally

From the project root:

```bash
npm run dev
```

This runs `cargo tauri dev --no-watch`, so Rust file watching and automatic application restarts are disabled.

Build a distributable app:

```bash
cd src-tauri
cargo tauri build
```

## Testing

Run all tests from one place (project root):

```bash
npm run test:all
```

Run Rust backend tests:

```bash
PYO3_PYTHON=python3.12 cargo test --manifest-path src-tauri/Cargo.toml
```

Run local-provider Rust tests and mocked Python inference tests separately:

```bash
npm run test:llm
npm run test:python
```

Run frontend unit tests:

```bash
cd ..
npm test
```

What is covered now:

- Rust unit tests for error handling, email transformation, spam-rating parsing, Python discovery, local-provider invocation, fetch helper logic, worker login error mapping, and DB enum conversions.
- Rust async DB integration tests for initialization, label serialization, storage population, pagination reads, sync-state tracking, and cleanup behavior.
- Mocked Python tests for model caching, device selection, warmup, inference, OOM handling, and cleanup without loading the real model.
- Frontend unit tests for spam-rating messages, shared error parsing, state defaults, mailbox/provider helpers, cache hydration/persistence, and DOM selector wiring.

## First-Run Flow

1. `check_app_status` decides where to route the user:
   - `setup`: no config yet
   - `login`: config exists, user not authenticated
   - `signed_in`: user already authenticated
2. Setup page collects:
   - IMAP server hostname
   - IMAP port
3. Login page collects:
   - Username (email)
   - Password
4. After successful login, app opens the main mailbox window and starts background sync.

## Startup Routing UX

- `login.html` now gates rendering behind an initialization check.
- A loading view is shown first, then routing happens based on `check_app_status`.
- If status is `setup` or `signed_in`, the app redirects without flashing the login form.

## Key Tauri Commands

- `check_app_status()`
- `config_setup(imap_server, imap_port)`
- `login(username, password)`
- `open_main_window()`
- `fetch_emails(...)`
- `rater(subject, email_from, body)`

## Mail Sync Model

- The backend worker keeps two IMAP sessions for concurrent mailbox work.
- Initial population batches inserts in chunks (`INITIAL_BATCH_SIZE = 100`) to avoid very large DB writes.
- Frontend pagination requests emails in pages (`INITIAL_BATCH_SIZE = 50`, `NEXT_BATCH_SIZE = 50`) and loads more on scroll.
- Ongoing refresh uses incremental UID-based sync via `mailbox_sync_state.last_uid`.

## Config and Data Storage

- App config directory: managed via Tauri `app_config_dir`
- Config file: `config.json`
- Mail database and cached data: under app config `data/`
- Credentials: stored through Apple Keychain integration on macOS

## Troubleshooting

- Stuck on setup/login:
  - Delete app config directory and relaunch to reset onboarding state.
- Login fails with auth errors:
  - Re-check username/password and provider IMAP access settings.
  - For Gmail/Yahoo, app-specific passwords may be required.
- RPC rating fails:
  - Confirm your RPC service is running and reachable at `RPC_SERVER`.
- Inbox appears capped on first visible load:
  - The frontend intentionally loads paged results first; scroll to load more.
  - If local mailbox data is partially populated and you need a clean re-bootstrap, clear app data and re-login.
- macOS asks for Keychain access when restarting:
  - RiiMail stores the email password as a generic Apple Keychain item. macOS controls authorization to read that item; see [Apple’s Keychain item documentation](https://developer.apple.com/documentation/security/keychain-items.md).
  - Choose **Always Allow** if you want macOS to remember access for the current app identity. Choosing **Allow** authorizes only the current access attempt, so a later restart can prompt again.
  - Development builds can be more likely to prompt because their executable/signing identity may differ from a packaged, signed release. This is an observed macOS behavior, not a guarantee that release builds will never prompt; verify the packaged build separately.
  - The Keychain account identifier is normalized to lowercase. If entries already exist for the same email with different casing, quit RiiMail and delete the stale casing variant in Keychain Access, then sign in again.

## Development Notes

- Frontend assets are served from `src/` (`frontendDist` in `src-tauri/tauri.conf.json`).
- The app starts in a compact auth window and expands after successful login.
- Logout clears local state and returns the UI to setup/login flow.

## Contributing & Security

Interested in contributing? Check out our [Contributing Guide](CONTRIBUTING.md) for details on how to get started. For reporting security vulnerabilities, please refer to our [Security Policy](SECURITY.md).

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.
