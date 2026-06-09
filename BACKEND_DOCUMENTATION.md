# RiiMail Backend Documentation

This document describes the backend architecture and behavior of the RiiMail application, focusing on the Tauri Rust backend under `src-tauri/src/`.

## Overview

The backend handles:

- application startup and Tauri window lifecycle
- configuration and initialization
- IMAP authentication and session management
- email synchronization and incremental refresh
- local SQLite storage for emails
- secure credential storage on macOS via Apple Keychain
- spam rating through an external LLM RPC service
- logout and cleanup

The main entrypoint is `src-tauri/src/main.rs`, which initializes tracing and calls `riimail_lib::run()`.

## Startup and App Bootstrap

### `src-tauri/src/main.rs`

- Loads environment variables with `dotenv::dotenv()`.
- Reads `OTLP_COLLECTOR_ENDPOINT` and `RUST_LOG`.
- Initializes tracing using `riimail_lib::tracing::init_subscriber`.
- Calls `riimail_lib::run().await`.

### `src-tauri/src/lib.rs`

- Defines global runtime state and `run()` logic.
- Creates communication channels for IMAP worker commands and login flows.
- Spawns the IMAP worker via `workers::worker(...)`.
- Manages application state objects:
  - `ArcSwap<AppState>` for initialized/ fresh state
  - `ImapClientChannelTx` for login requests
  - `ReturningUserImapClientChannelTx` for returning-user login requests
  - `UnboundedSender<ImapCommand>` for runtime commands
  - `SharedFetchManager` for cancellation tokens
- Sets up the native application menu and logout event.
- Configures window resizing behavior based on startup state.
- Registers Tauri commands:
  - `check_app_status`
  - `config_setup`
  - `fetch_emails_handler`
  - `login`
  - `open_main_window`
  - `rater`
  - `refresh_emails_handler`
  - `logout`

### Global Flags and State

- `LOGGED_IN`: true when the user is authenticated.
- `LOGGING_IN`: true while login is in progress.
- `FETCH_MANAGER`: provides `logout_token` and `refresh_token` for canceling tasks.

## Configuration and Initialization

### `src-tauri/src/config.rs`

- Defines `Config` with:
  - `rpc_server`
  - `imap_server`
  - `imap_port`
  - `sqlite_db`
  - `accounts`
- Loads and validates `config.json` through `Config::init(path)`.
- Defines `AppState` enum with `Fresh` and `Initialized` variants.
- Defines `InitializedState` which contains:
  - `rpc_llm_client`
  - `sqlite_pool`
  - `apple_keychain_manager`
- Provides `init_rpc(rpc_server)` to connect to an external tarpc RPC server.

### `src-tauri/src/handlers/configuration.rs`

- `config_setup(app, imap_server, imap_port)`:
  - uses environment variables `RPC_SERVER` and `SQLITE_DB`
  - writes `config.json`
  - initializes the SQLite database via `db::init_db`
- `check_app_status(app)`:
  - returns app initialization state:
    - `InitStatus::Setup`
    - `InitStatus::Login`
    - `InitStatus::SignedIn`
    - `InitStatus::ReturningSigned`
  - attempts returning-user login if credentials are stored in Apple Keychain.
  - resizes the app window for compact auth or expanded main UI.
- `open_main_window(current_window)`:
  - expands the current window after successful login.

## Authentication and Login Flow

### `src-tauri/src/handlers/login.rs`

- `login(app, username, password)` performs:
  - sets `LOGGING_IN` to true.
  - loads `config.json`.
  - appends the username to `config.accounts` and saves it.
  - calls `handle_initialization(...)`:
    - selects mail provider from IMAP server
    - opens SQLite pool
    - initializes RPC LLM client
    - creates Apple Keychain manager
  - sends `ImapClientConfig` to the IMAP worker.
  - waits for a oneshot response from the worker.
  - on success:
    - stores password in Apple Keychain on macOS
    - sends initial `ImapCommand::FetchEmails` for `MailBox::Inbox`
    - updates `AppState::Initialized`
    - sets `LOGGED_IN` true and `LOGGING_IN` false

### `src-tauri/src/auth/apple_keychain_access.rs`

- Stores and retrieves passwords on macOS.
- Used for returning-user login and secure credential storage.
- On non-macOS, operations return a keychain error.

### `src-tauri/src/auth/login.rs`

- Performs IMAP login using `async_imap`.
- Converts the IMAP client to an authenticated session.

## IMAP Worker and Background Tasks

### `src-tauri/src/workers/imap_session.rs`

- The IMAP worker is the backend's central long-running task.
- It listens to:
  - `imap_client_channel_rx` for first-time login requests
  - `imap_client_returning_user_channel_rx` for returning users
  - `imap_cmd_channel_rx` for fetch/refresh/logout commands
- Maintains internal state:
  - `initialized_session`
  - `initialized_session_background`
  - `pool` (SQLite pool)
  - `providers`
  - `initial_fetch_completed`
  - `database_initialized`
  - `logging_out`
- Creates a periodic refresh ticker every 60 seconds.

### Worker Command Handling

- `ImapCommand::FetchEmails(mailbox, provider, cancel_token)`:
  - initial sync for a mailbox
  - rejects duplicate initial fetches
  - requires both primary and background sessions
  - calls `email_cache::fetch_emails`
- `ImapCommand::RefreshEmails(mailbox, provider, login_result_tx)`:
  - performs incremental refresh of latest messages
  - calls `email_cache::fetch_latest`
  - sends `RefreshSummary` back via oneshot channel
- `ImapCommand::Logout`:
  - logs out both IMAP sessions
  - clears pool and state
  - sets `LOGGED_IN` false
  - sends completion over logout channel

### Automatic Periodic Refresh

- Every 60 seconds, if the database is initialized and not logging out:
  - refreshes Inbox and Sent for each provider
  - uses `email_cache::fetch_latest(...)`
  - runs both inbox and sent refreshes concurrently

### Login Initialization

- When receiving an IMAP config, the worker:
  - establishes an IMAP client and authenticates it
  - stores the authenticated session as `SharedImapSession`
  - may also create a secondary background session
  - stores SQLite pool reference
  - notifies caller when login completes

## IMAP Connection Management

### `src-tauri/src/imap.rs`

- `init_imap_client(imap_server, imap_port)`:
  - opens TCP connection to IMAP host
  - wraps it in TLS using `async_native_tls`
  - returns an `async_imap::Client`
- Defines `ImapCommand` enum:
  - `Logout`
  - `FetchEmails(mailbox, provider, cancel_token)`
  - `RefreshEmails(mailbox, provider, result_sender)`
- Defines `RefreshSummary` with:
  - `new_emails_count`
  - `total_emails`

## Email Fetching and Storage

### `src-tauri/src/email_cache/fetcher.rs`

- `fetch_emails(...)`:
  - initial mailbox synchronization
  - avoids fetching if DB already contains mailbox records
  - selects mailbox and fetches all messages with `UID FETCH 1:*`
  - inserts messages into SQLite in batches of 100
  - updates folder counts and last UID state
  - spawns background Sent mailbox fetch when syncing Inbox
- `fetch_latest(...)`:
  - incremental refresh only for new UIDs
  - reads last synced UID from DB
  - searches for newer messages and fetches them
  - inserts new rows and updates last UID
- Uses cancellation tokens to abort in-progress operations cleanly.

### `src-tauri/src/email_cache/mod.rs`

- Defines the email data model:
  - `Email`
  - `CompleteEmail`
  - `FrontendEmail`
- Converts raw IMAP fetch results into structured messages.
- Parses email headers, preview, html body, text body, attachments, and labels.
- Maps Gmail labels into frontend folder and starred state.
- Normalizes text and produces frontend-ready fields.

## SQLite Database Schema and Utilities

### `src-tauri/src/db/mod.rs`

- Creates local SQLite database under app config directory.
- Builds provider-specific mailbox tables:
  - `gmail_INBOX`, `gmail_Sent`
  - `yahoo_INBOX`, `yahoo_Sent`
- Creates `total_emails` and `mailbox_sync_state` tables.
- Adds indexes on `uid` and `date`.
- Initializes the DB with PRAGMA configuration.

### `src-tauri/src/db/emails_db.rs`

- `populate_storage(...)` inserts email rows with deduplication.
- `get_emails(...)` returns messages for frontend pagination.
- `populate_inbox_folder_count(...)` and `populate_sent_folder_count(...)`
- `get_email_count(...)`
- `check_email_db_empty(...)`
- `get_last_uid(...)` and `set_last_uid(...)`
- `cleanup(...)` removes DB file(s).

## Frontend Communication Endpoints

### `src-tauri/src/handlers/email.rs`

- `fetch_emails_handler(app, min_range, max_range, mailbox, provider)`:
  - loads emails from SQLite
  - converts them to `FrontendEmail`
- `refresh_emails_handler(app, mailbox, provider)`:
  - requests incremental refresh from the worker
  - waits for a `RefreshSummary`
  - fetches newly added emails from SQLite
  - returns message list plus counts

### `src-tauri/src/handlers/rater.rs`

- `rater(app, subject, email_from, body)`:
  - creates an `EmailRequest`
  - forwards it to the LLM RPC client
  - returns `SpamRating`

### `src-tauri/src/handlers/menu.rs`

- `logout_with_state(app)`:
  - sends `ImapCommand::Logout`
  - cancels fetch and refresh tokens
  - deletes `config.json`
  - waits for worker confirmation
  - cleans up database files
  - resets `AppState::Fresh`

## LLM / Spam Rating Integration

### `src-tauri/src/llm/rpc/mod.rs`

- Defines the RPC service trait.
- Uses `tarpc` and `rpc_agent` types.
- `caller(rpc_client, email)` sends a message to the external agent server.

### `src-tauri/src/llm/rpc/call.rs`

- Sends an RPC request with a 120-second deadline.
- Retries on connection errors.
- Detects inference errors like `MODEL_OOM` and retries.
- Converts returned JSON into `SpamRating`.

### `src-tauri/src/llm/mod.rs`

- Defines provider abstractions for:
  - `ollama`
  - `openai`
  - `sagemaker`
  - `local`
- Defines `EmailRequest`, `SpamRating`, and `Label`.
- Provides initialization helper functions.

### `src-tauri/src/llm/providers/local_inference.rs`

- Uses PyO3 to import a Python inference module.
- Calls `predict(prompt)` in Python and returns JSON.
- Supports local model inference from `python/local_inference.py`.

## Local Python Inference

### `python/local_inference.py`

- Loads a local Transformers model from `python/merged-model-new`.
- Selects device priority:
  - MPS on Apple Silicon
  - CUDA if available
  - CPU fallback
- Runs `predict(text)` and returns:
  - `label`
  - `score`
- Handles OOM and fatal errors with JSON-coded responses.

## Environment Variables

- `OTLP_COLLECTOR_ENDPOINT` — tracing exporter endpoint.
- `RUST_LOG` — logging level.
- `RPC_SERVER` — remote RPC server address, default `0.0.0.0:5500`.
- `SQLITE_DB` — local SQLite filename, default `emails.db`.

## Command Flow Summary

1. App launches.
2. `run()` starts the IMAP worker and Tauri app.
3. Frontend calls `check_app_status`.
4. If config missing:
   - UI shows setup, and `config_setup` is called.
5. If config exists but not authenticated:
   - UI shows login page.
   - `login` performs IMAP auth and starts sync.
6. If returning user credentials are stored:
   - `check_app_status` attempts background login.
7. On successful login:
   - app state becomes initialized
   - email sync begins
   - main window opens/expands
8. Frontend uses `fetch_emails_handler` and `refresh_emails_handler` for mailbox data.
9. `rater` sends email content to the LLM RPC backend.
10. Logout triggers `logout_with_state`, session cleanup, and config deletion.

## Backend Responsibilities

The backend is responsible for:

- managing Tauri application state and window configuration
- persisting app config and account metadata
- handling secure credential storage on macOS
- controlling IMAP session lifecycle and background sync tasks
- storing fetched emails in local SQLite
- exposing email fetch and refresh APIs to the frontend
- performing spam rating through an LLM RPC path
- cleaning up local config and database on logout
