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

### Local inference runtime

The macOS arm64 release is self-contained. Its app bundle includes:

- the classification model from `local-model/`
- `llm/local_inference.py`
- a checksum-pinned, relocatable Python 3.12 runtime
- pinned binary wheels for PyTorch, Transformers, Tokenizers, Safetensors, and their dependencies

Users do **not** need Homebrew, Python, a virtualenv, Python packages, or network access for local spam/phishing inference. At startup, RiiMail replaces inherited Python environment variables, initializes only the bundled runtime, restricts `sys.path` to app resources, enables Hugging Face offline mode, and rejects `torch` or `transformers` imports outside the app bundle.

The release targets Apple Silicon and requires macOS 14 or newer because the pinned PyTorch wheel has a macOS 14 deployment target.

#### Download the local model

The model is intentionally excluded from the GitHub repository because of its size. Before running the app or building a package, download [`seuun/spam-rater-local-model`](https://huggingface.co/seuun/spam-rater-local-model) into `local-model/` at the repository root.

Using Git LFS:

```bash
git lfs install
git clone https://huggingface.co/seuun/spam-rater-local-model local-model
```

Alternatively, using the Hugging Face CLI:

```bash
hf download seuun/spam-rater-local-model --local-dir local-model
```

Run the commands from the RiiMail repository root. If `local-model/` already exists, remove or move it before cloning rather than creating a nested `local-model/spam-rater-local-model/` directory. Confirm that the actual model data—not Git LFS pointer files—was downloaded:

```bash
test -f local-model/config.json
test -f local-model/model.safetensors
ls -lh local-model/model.safetensors
```

`model.safetensors` should be hundreds of megabytes; a tiny text file indicates that only a Git LFS pointer was downloaded. The build stops with an error if `local-model/config.json` is missing. The directory is ignored by Git and should not be committed to this repository.

Runtime inputs are tracked in:

- `packaging/macos-arm64/runtime.lock.json`
- `packaging/macos-arm64/requirements.txt`
- `src-tauri/tauri.macos.conf.json`

## Environment Variables

Copy the example environment file in the repository root:

```bash
cp .env.example .env
```

The example contains:

```bash
RUST_LOG=info
TELEMETRY_OPT_IN=false
OTLP_COLLECTOR_ENDPOINT=http://0.0.0.0:4317
```

Notes:

- `RUST_LOG` can be raised to `debug` during local troubleshooting.
- `TELEMETRY_OPT_IN` defaults to `false`. Set it to `true` to export traces to the configured OTLP collector.
- `OTLP_COLLECTOR_ENDPOINT` is only used when `TELEMETRY_OPT_IN=true`.

## Run Locally

From the project root:

```bash
npm run dev
```

This runs `packaging/macos-arm64/dev.sh`. The wrapper prepares or reuses the pinned Python runtime, configures PyO3 to link against its `libpython3.12.dylib`, exposes that dylib to the debug executable, and then starts `cargo tauri dev --no-watch`. The first run downloads the runtime and pinned wheels; later runs reuse the staged files.

Use `npm run dev` rather than invoking `cargo tauri dev` directly. The direct Cargo command does not provide the PyO3 link configuration and can produce a debug executable linked to Homebrew or another machine-local Python installation.

After downloading the model as described above, build the self-contained Apple Silicon DMG from the project root:

```bash
npm run build:macos-arm64
```

The first build downloads the checksum-pinned standalone Python archive and pinned wheels. The wrapper stages the runtime, links PyO3 to the bundled dylib, builds and signs the app, audits every Mach-O dependency, runs real offline model inference from the packaged app, and verifies the DMG checksum.

Output:

```text
src-tauri/target/aarch64-apple-darwin/release/bundle/dmg/RiiMail_0.1.0_aarch64.dmg
```

The generated DMG is ad-hoc signed for local testing. Public distribution still requires your Apple Developer ID signing identity and notarization to avoid Gatekeeper warnings on downloaded builds.

## Testing

Run all tests from one place (project root):

```bash
npm run test:all
```

Run Rust backend tests after staging the pinned runtime:

```bash
python3 packaging/macos-arm64/scripts/prepare-runtime.py
npm run test:backend
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

- Rust unit tests for error handling, email transformation, spam-rating parsing, bundled-runtime path validation, fetch helper logic, worker login error mapping, and DB enum conversions.
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
