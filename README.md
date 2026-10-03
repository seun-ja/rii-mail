# RiiMail (Email Desktop Client)

RiiMail is a Tauri 2 desktop app for triaging email with IMAP sync and AI-assisted phishing/spam checks.

## Quick Start

To get started, you will need to configure your local environment:

1. Clone this repository.
2. Create a `.env` file in the root (see [Environment Variables](#environment-variables) below).
3. Run the application using `cargo tauri dev`.

## What It Does

- Connects to an IMAP account (for example Gmail or Yahoo)
- Syncs and caches mailbox data locally (Inbox/Sent)
- Displays a desktop inbox experience
- Runs a spam/phishing rating workflow through an RPC AI service
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
      llm/                    # LLM provider/rpc integration
```

## Prerequisites

- Rust toolchain (stable) via rustup
- Tauri CLI:

```bash
cargo install tauri-cli
```

On macOS, ensure Xcode Command Line Tools are installed.

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

- `RPC_SERVER` must point to your running RPC agent service.
- `RUST_LOG` can be raised to `debug` during local troubleshooting.
- `TELEMETRY_OPT_IN` defaults to `false`. Set it to `true` to export traces to the configured OTLP collector.
- `OTLP_COLLECTOR_ENDPOINT` is only used when `TELEMETRY_OPT_IN=true`.

## Run Locally

From the project root:

```bash
cd src-tauri
cargo tauri dev
```

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
cd src-tauri
PYO3_PYTHON=/opt/homebrew/bin/python3.12 cargo test
```

Run frontend unit tests:

```bash
cd ..
npm test
```

What is covered now:

- Rust unit tests for error handling, email transformation, fetch helper logic, worker login error mapping, and DB enum conversions.
- Rust async DB integration tests for initialization, storage population, pagination reads, sync-state tracking, and cleanup behavior.
- Frontend unit tests for shared error parsing, state defaults, mailbox/provider helpers, cache hydration/persistence, and DOM selector wiring.

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

## Development Notes

- Frontend assets are served from `src/` (`frontendDist` in `src-tauri/tauri.conf.json`).
- The app starts in a compact auth window and expands after successful login.
- Logout clears local state and returns the UI to setup/login flow.

## Contributing & Security

Interested in contributing? Check out our [Contributing Guide](CONTRIBUTING.md) for details on how to get started. For reporting security vulnerabilities, please refer to our [Security Policy](SECURITY.md).

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.
