# RiiMail Backend Documentation

This document describes the **current backend that is actively used by the app today**.

It is based on the Rust/Tauri backend under `src-tauri/src/` and the frontend call sites under `src/`.

## Scope of this document

This documentation covers the backend paths that are **currently wired into the running app**:

- Tauri app startup and runtime state
- setup and login
- returning-user auto-login
- IMAP session lifecycle
- initial mailbox sync and manual refresh
- SQLite persistence
- SMTP send flow
- AI email draft generation
- spam/phishing rating via RPC
- logout and cleanup
- async/multi-threaded execution model

This document intentionally **does not treat dormant or not-currently-wired code as active backend behavior**. In particular:

- the repo contains Python local inference code under `python/` and local-inference provider code in `llm/`, but the active Tauri backend does **not** call that path today
- `Drafts` and `Trash` exist in enums, but the active backend mailbox sync flow only uses `INBOX` and `Sent`
- `InitStatus::ReturningSigned` exists, but the current auto-login path returns `SignedIn`

View the high-level diagram here: `high_level.puml`

---

## 1. Backend at a glance

### Main backend entrypoints

- `src-tauri/src/main.rs`
- `src-tauri/src/lib.rs`

### Main backend responsibilities

The active backend is responsible for:

- bootstrapping the Tauri app
- loading config from the app config directory
- establishing IMAP connections
- fetching and caching emails into SQLite
- exposing mailbox fetch/refresh commands to the frontend
- securely retrieving/storing passwords through Apple Keychain on macOS
- sending outbound emails through SMTP
- generating email drafts through the in-process LLM agent
- rating emails for spam/phishing through an external RPC service
- cleaning up config and cached mail on logout

### Current frontend-facing backend surface

The frontend currently invokes these Tauri commands:

- `check_app_status`
- `config_setup`
- `login`
- `open_main_window`
- `inbox_email_populated`
- `sent_email_populated`
- `inbox_intial_email_populated`
- `sent_intial_email_populated`
- `fetch_emails`
- `refresh_emails_handler`
- `send_email`
- `email_generator`
- `rater`

Logout is currently driven through the **native menu event** in `src-tauri/src/lib.rs`, which calls `logout_with_state(...)`.

---

## 2. Runtime architecture

### `src-tauri/src/main.rs`

`main.rs` is thin:

- loads `.env` via `dotenv`
- reads:
  - `OTLP_COLLECTOR_ENDPOINT`
  - `RUST_LOG`
- initializes tracing via `riimail_lib::tracing::init_subscriber(...)`
- starts the app via `riimail_lib::run().await`

### `src-tauri/src/lib.rs`

`run()` builds the real backend runtime. It:

- creates channel pairs for:
  - first-time login IMAP config
  - returning-user IMAP config
  - IMAP commands
  - logout completion
  - first-page population notifications
- starts the IMAP worker via `workers::worker(...)`
- registers shared application state with `tauri::Builder`
- registers the Tauri commands used by the frontend
- configures the app menu and logout handler
- adjusts window size depending on whether local backend state already exists

### Shared state registered into Tauri

`lib.rs` manages these key runtime objects:

- `ArcSwap<AppState>`
  - global initialized vs fresh app state
- `ImapClientChannelTx`
  - first-time login config sender
- `ReturningUserImapClientChannelTx`
  - returning-user login config sender
- `UnboundedSender<ImapCommand>`
  - runtime IMAP command sender
- `SharedFetchManager`
  - shared cancellation-token holder
- `InboxPopulateUpdateState`
- `SentPopulateUpdateState`
- `InitialDbPopulation`
  - stores oneshot receivers used to notify the frontend when the first usable page has landed in SQLite

### Global flags

Defined in `src-tauri/src/lib.rs`:

- `LOGGED_IN: LazyLock<Arc<Mutex<bool>>>`
- `LOGGING_IN: LazyLock<Arc<Mutex<bool>>>`
- `FETCH_MANAGER: LazyLock<Arc<FetchManager>>`

These are used to coordinate login/logout state and cancellation of long-running fetch work.

---

## 3. Configuration and local state

### Config file location

The backend stores config in the Tauri app config directory as:

- `config.json`

The SQLite database lives under:

- `data/<sqlite_db>`

Both paths are resolved from `app.path().app_config_dir()`.

### `src-tauri/src/config.rs`

The active config shape is:

- `rpc_server_url`
- `imap_server_url`
- `smtp_relay_url`
- `imap_port`
- `sqlite_db`
- `accounts`
- `provider`
- `default_model`
- `api_key`

### What is actively used from config today

Actively consumed by the current backend:

- `rpc_server_url`
  - used by spam/phishing RPC client setup
- `imap_server_url`
  - used to connect/login to IMAP
- `smtp_relay_url`
  - used to build the SMTP transport
- `imap_port`
  - used by IMAP TCP/TLS setup
- `sqlite_db`
  - used to locate the local SQLite file
- `accounts`
  - the first account is used for return-login lookup and SMTP sender identity
- `default_model`
  - used by the draft-generation LLM agent
- `api_key`
  - passed into the draft-generation LLM agent

Present in config but **not meaningfully consumed by the active backend flow**:

- `provider`
  - written during setup, but the active Rust flow derives provider from the IMAP hostname instead

### Provider resolution

`src-tauri/src/handlers/mod.rs` currently resolves provider like this:

- if the IMAP server contains `yahoo` -> `Provider::Yahoo`
- otherwise -> `Provider::Gmail`

So provider is inferred from the IMAP host, not from the saved `config.provider` field.

---

## 4. Setup, login, and startup flow

## 4.1 Setup flow

### `src-tauri/src/handlers/configuration.rs::config_setup`

This is called from `src/setup.js`.

It:

1. reads environment defaults
2. builds `Config`
3. determines provider from the IMAP server string
4. initializes the SQLite schema for that provider via `db::init_db(...)`
5. writes `config.json` into the app config directory

### Environment-backed defaults used by setup

`config_setup` currently reads:

- `RPC_SERVER` default: `0.0.0.0:5500`
- `SQLITE_DB` default: `emails.db`
- `SMTP_RELAY_URL` default: `smtp.mail.yahoo.com`
- `MODEL` default: `gemma4:e4b`
- `LLM_API_KEY` default: `ollama`
- env var named `ollama` default: `ollama`

That last value is saved into `Config.provider`, but as noted above it is not part of the active provider-selection path.

## 4.2 Startup status check

### `src-tauri/src/handlers/configuration.rs::check_app_status`

This is called from `src/login.js` on app startup.

Current behavior:

- if `config.json` is missing or invalid:
  - resize window to compact auth mode
  - return `InitStatus::Setup`
- if config exists:
  - try to retrieve password from Apple Keychain for the first account in `config.accounts`
- if no stored password is found:
  - resize to compact auth mode
  - return `InitStatus::Login(None)`
- if stored password exists:
  - create SQLite pool
  - build SMTP transport
  - send IMAP config to the returning-user login channel
  - wait for worker login result
  - build initialized app state
  - expand the window
  - return `InitStatus::SignedIn`

### Important current note

Although `InitStatus::ReturningSigned` exists in the enum, the active auto-login path currently returns:

- `InitStatus::SignedIn`

## 4.3 Explicit login flow

### `src-tauri/src/handlers/login.rs::login`

This is called from `src/login.js` after the user submits credentials.

Current login flow:

1. set `LOGGING_IN = true`
2. load `config.json`
3. append the username to `config.accounts`
4. save `config.json`
5. initialize backend state via `handle_initialization(...)`
6. send `ImapClientConfig` to the worker over the first-time login channel
7. await a oneshot login result from the worker
8. on success, store password in Apple Keychain
9. create a fresh logout cancellation token hierarchy
10. send `ImapCommand::FetchEmails(MailBox::Inbox, provider, fetch_token)`
11. build `AppState::Initialized(...)`
12. set `LOGGING_IN = false` and `LOGGED_IN = true`

### `handle_initialization(...)`

This helper creates the initialized service set:

- provider from IMAP host
- SQLite pool
- RPC spam-rating client
- Apple Keychain manager
- SMTP transport with current credentials
- LLM email-generation agent
- account list (currently one active account)

### Current account model

The config has an `accounts: Vec<String>`, but the active backend behaves like a **single-account app** in practice:

- first stored account is used for return-login lookup
- first account is used as the SMTP sender identity
- `send_email` reads `accounts[0]`

---

## 5. IMAP and email sync architecture

### Relevant files

- `src-tauri/src/imap.rs`
- `src-tauri/src/workers/imap_session.rs`
- `src-tauri/src/email_cache/fetcher.rs`
- `src-tauri/src/email_cache/mod.rs`
- `src-tauri/src/db/mod.rs`
- `src-tauri/src/db/emails_db.rs`

## 5.1 IMAP connection setup

### `src-tauri/src/imap.rs::init_imap_client`

The backend:

1. opens a TCP connection with `tokio::net::TcpStream`
2. wraps it with `async_native_tls`
3. creates an `async_imap::Client`

The active connection is therefore:

- async TCP
- async TLS
- async IMAP session

## 5.2 Worker model

### `src-tauri/src/workers/imap_session.rs::worker`

The worker is the main long-lived background task for mail operations.

It owns runtime mail state such as:

- primary authenticated IMAP session
- background authenticated IMAP session
- SQLite pool reference
- refresh ticker
- logout state

It listens on:

- first-time login config channel
- returning-user login config channel
- IMAP command channel

## 5.3 Current mailbox scope

The active backend sync path currently supports:

- `INBOX`
- `Sent`

`Drafts` and `Trash` exist in the `MailBox` enum, but they are not part of the active sync implementation:

- initial population for those variants is `unimplemented!("Coming soon")`
- current frontend sync calls only `INBOX` and `Sent`

## 5.4 Initial sync behavior

### Trigger point

After login, the frontend/backend flow only sends one explicit initial fetch command:

- `ImapCommand::FetchEmails(MailBox::Inbox, provider, token)`

### What actually happens next

In `email_cache::fetch_emails(...)`:

- the requested mailbox is fetched on the primary session
- if the requested mailbox is `Inbox` and a background session exists, the backend also spawns a background task to fetch `Sent`

So in the current app:

- login triggers an Inbox fetch command
- that Inbox fetch also starts a concurrent Sent fetch in the background

### Skip behavior when data already exists

`fetch_emails(...)` checks whether the mailbox table already has rows.

If rows already exist, it returns early with `FetchResult::Populated` and does not run a full IMAP bootstrap for that mailbox.

That means the expensive initial fetch is effectively only for an empty mailbox table.

## 5.5 Initial sync mechanics

### `src-tauri/src/email_cache/fetcher.rs::handle_email_population`

For full mailbox population, the backend:

1. selects the mailbox with `session.select(...)`
2. calls:
   - `UID FETCH 1:* (UID FLAGS ENVELOPE INTERNALDATE BODY.PEEK[])`
3. streams IMAP messages incrementally
4. converts each message into `StandardEmail`
5. batches inserts into SQLite
6. records the highest UID seen
7. updates folder counts
8. persists `last_uid` into `mailbox_sync_state`

### Insert batching

The initial batch size is currently:

- `100`

Messages are inserted in chunks of 100 via `populate_storage(...)`.

### Important UI-visible detail

The "initial population complete" event does **not** mean the entire mailbox has finished syncing.

Current behavior:

- once the first batch is inserted into SQLite
- the worker sends a oneshot notification
- the frontend can render the first usable page immediately
- the rest of the mailbox may still be streaming in afterward

This is why startup can feel responsive even while a large mailbox is still being imported.

## 5.6 Incremental refresh behavior

### `src-tauri/src/email_cache/fetcher.rs::fetch_latest`

Manual refresh uses the stored last UID.

The backend:

1. selects the mailbox
2. reads `last_uid` from `mailbox_sync_state`
3. searches IMAP using `last_uid + 1:*`
4. fetches only new UIDs
5. inserts only those messages into SQLite
6. updates `last_uid`
7. returns:
   - count of new emails
   - total mailbox size reported by IMAP

This is the active refresh path used by the frontend refresh button.

---

## 6. Deep dive: multi-threading, async tasks, and concurrency

This is the most important section if you want to understand how the backend actually runs.

## 6.1 It is async-first, not thread-per-feature

The backend uses Tokio throughout.

Because `main` is annotated with `#[tokio::main]`, the app runs on a Tokio runtime. In practice, the system is best understood as:

- **one async application**
- with **multiple concurrent tasks**
- scheduled by Tokio across runtime worker threads

So when people say "multi-threaded" here, the accurate description is:

- the runtime may use multiple OS threads
- but the code is structured primarily around **async tasks, channels, mutexes, and cancellation tokens**
- not around manually created OS threads

## 6.2 The key concurrency primitive is the IMAP worker task

`worker(...)` immediately does:

- `tokio::spawn(async move { ... })`

That creates one long-lived background task which acts like a mail coordinator.

Inside that task, the worker waits on multiple inputs using `tokio::select!`:

- login config messages
- returning-user login config messages
- IMAP commands
- a periodic ticker

This gives the backend a **single coordination loop** for IMAP lifecycle state.

## 6.3 Why there are two IMAP sessions

The worker holds:

- `initialized_session`
- `initialized_session_background`

Both are `SharedImapSession`, which is:

- `Arc<Mutex<Session<TlsStream<TcpStream>>>>`

That means:

- session ownership is shared with `Arc`
- only one async user at a time can operate on a given session because of the `Mutex`

### Why this matters

A single IMAP session is stateful:

- mailbox selection changes session state
- concurrent commands on one session would conflict

So the backend uses **two authenticated sessions**:

- primary session
- background session

That allows current active behavior such as:

- fetching `INBOX` on one session
- fetching `Sent` at the same time on the second session

without the two mailbox selections stepping on each other.

## 6.4 Current concurrency during login/bootstrap

After login succeeds, the app sends only one `FetchEmails` command for `INBOX`.

Inside `fetch_emails(...)`:

- the Inbox fetch runs on the primary session
- a background task is spawned with `tokio::spawn(...)`
- that background task fetches `Sent` on the second IMAP session

So current startup concurrency looks like this:

1. login finishes
2. worker already has two authenticated IMAP sessions
3. Inbox bootstrap starts
4. Sent bootstrap starts in a spawned async task
5. both write into SQLite concurrently
6. frontend receives progress and first-page-ready notifications

## 6.5 Current concurrency during manual refresh

When the frontend calls `refresh_emails_handler(...)`:

1. a oneshot response channel is created
2. the handler sends `ImapCommand::RefreshEmails { ... }`
3. the worker handles the command
4. the worker calls `fetch_latest(...)`
5. the worker responds on the oneshot channel with `RefreshSummary`
6. the handler reads the newly inserted rows from SQLite and returns them to the frontend

This is command/response concurrency, not background fire-and-forget.

## 6.6 Current progress/event model

Progress tracking uses:

- `AtomicU8` values wrapped in shared state
- polling commands that emit frontend events every 100ms

### How Inbox/Sent progress works

During initial population:

- each streamed message updates a percentage based on IMAP `exists`
- that percentage is stored into:
  - `INBOX_POPULATE_UPDATE`
  - `SENT_POPULATE_UPDATE`

Meanwhile, the commands:

- `inbox_email_populated`
- `sent_email_populated`

run their own loop:

- read the atomic percentage
- emit Tauri events:
  - `inbox-population-progress`
  - `sent-population-progress`
- stop when progress reaches 100

So the UI gets progress updates by **polling shared progress state and re-emitting events**, not by direct push from the IMAP fetch loop.

## 6.7 First-page-ready notifications use oneshot channels

`InitialDbPopulation` stores two oneshot receivers:

- one for Inbox
- one for Sent

The fetcher sends these exactly once when the first usable batch is stored.

The commands:

- `inbox_intial_email_populated`
- `sent_intial_email_populated`

await those receivers and then emit:

- `inbox-initial-population-complete`
- `sent-initial-population-complete`

This is how the frontend knows when it can start rendering data from SQLite without waiting for the full mailbox import to finish.

## 6.8 Cancellation model

Cancellation is handled with `tokio_util::sync::CancellationToken`.

### Stored tokens

`FetchManager` contains:

- `logout_token`
- `refresh_token`

### Current use

On login:

- the logout parent token is replaced with a new token
- a child token is passed into the initial fetch command

For refresh:

- the worker creates a child of `refresh_token`
- that token is passed into `fetch_latest(...)`

On logout:

- both parent tokens are cancelled
- ongoing fetch/refresh work can stop cooperatively

### What cancellation looks like in fetch code

The fetcher repeatedly checks:

- `cancel_token.is_cancelled()`
- `cancel_token.cancelled()` in `tokio::select!`

When cancellation wins, the fetcher returns:

- `Error::ThreadCancel`

So cancellation is cooperative and explicit throughout long-running mail operations.

## 6.9 Automatic periodic refresh: present in code, not active in the current flow

The worker contains a 60-second ticker and code intended to periodically refresh mailboxes.

However, in the current implementation shown in `src-tauri/src/workers/imap_session.rs`:

- `database_initialized` starts as `false`
- `providers` starts empty
- this file does not currently populate those values before the ticker branch is gated on them

So the documented, code-accurate statement for the current backend is:

- **manual refresh is active**
- **automatic periodic refresh scaffolding exists in the worker**
- **but it is not effectively enabled by the current code path**

That is why the frontend’s explicit refresh flow is the reliable active mechanism today.

---

## 7. SQLite storage model

### `src-tauri/src/db/mod.rs`

The backend creates a SQLite database under the app config directory and enables:

- `PRAGMA journal_mode = WAL`
- `PRAGMA synchronous = NORMAL`
- `PRAGMA foreign_keys = ON`

### Tables actively used

For each provider, the current schema creates:

- `<provider>_INBOX`
- `<provider>_Sent`

Examples:

- `gmail_INBOX`
- `gmail_Sent`
- `yahoo_INBOX`
- `yahoo_Sent`

It also creates:

- `total_emails`
- `mailbox_sync_state`

### Purpose of each table

#### Mailbox tables

Store:

- `uid`
- `date`
- raw `body`
- serialized `labels`

#### `total_emails`

Stores per-provider mailbox counts for:

- `INBOX`
- `Sent`

#### `mailbox_sync_state`

Stores:

- `mailbox`
- `last_uid`

This is what powers incremental refresh.

### Storage write behavior

`populate_storage(...)`:

- sorts emails newest-first by date before insert
- inserts with `INSERT OR IGNORE`
- writes inside a DB transaction

That means duplicate UID inserts are ignored safely.

### Read behavior

`get_emails(...)`:

- reads a page of rows ordered by descending datetime
- converts stored rows back into `CompleteEmail`
- then handlers map them into `FrontendEmail`

---

## 8. Email parsing and frontend shaping

### `src-tauri/src/email_cache/mod.rs`

The backend stores mostly raw mail content, then reconstructs frontend-friendly objects when reading from SQLite.

### Main mail representations

- `StandardEmail`
  - near-storage/transport form
- `CompleteEmail`
  - parsed message with content details
- `FrontendEmail`
  - shape returned to the frontend

### What gets extracted

From parsed email bodies, the backend derives:

- subject
- sender name
- sender email
- preview text
- html body
- text body
- attachment ids
- part ids
- raw full message

### Folder mapping behavior

`map_folder_and_starred(...)` interprets labels into frontend-friendly values like:

- `inbox`
- `sent`
- `archive`
- starred flag

### Important current limitation

The backend only actively syncs `INBOX` and `Sent`.

The frontend does show a `Trash`/archive-style folder, but that is currently derived from cached/frontend state and label mapping rather than from a real backend Trash mailbox sync API.

---

## 9. Frontend-facing commands and what they do now

## 9.1 Setup and auth

### `check_app_status`

Used by `src/login.js`.

Returns whether the app should:

- go to setup
- stay on login
- enter signed-in mode via successful auto-login

### `config_setup`

Used by `src/setup.js`.

Writes config and prepares the DB schema.

### `login`

Used by `src/login.js`.

Authenticates, stores credentials, initializes app state, and kicks off initial sync.

### `open_main_window`

Used by `src/login.js` after successful login.

Reuses the current auth window, expands it, and navigates it to the main app UI.

## 9.2 Sync and mailbox data

### `fetch_emails`

Used by `src/mail-app/sync.js`.

Reads a page of emails from SQLite for a mailbox/provider pair.

Important current behavior:

- it is a DB read API
- it does not itself trigger an IMAP refresh
- the frontend uses it for pagination and bootstrap reads

### `refresh_emails_handler`

Used by `src/mail-app/sync.js`.

Requests incremental IMAP refresh, then returns the newly added rows plus summary counts.

### `inbox_email_populated` / `sent_email_populated`

Used by `src/mail-app/init.js`.

Continuously emit progress events during initial bootstrap.

### `inbox_intial_email_populated` / `sent_intial_email_populated`

Used by `src/mail-app/init.js`.

Emit completion events when the first renderable batch is written.

## 9.3 Outbound email and AI

### `send_email`

Used by `src/mail-app/init.js`.

Builds a message with Lettre and sends it through the initialized SMTP transport.

Current behavior:

- sender is derived from `accounts[0]`
- `to`, `cc`, and `bcc` are validated and converted into `Mailbox`
- `contentType` is parsed, defaulting to plain text if parsing fails

### `email_generator`

Used by `src/mail-app/init.js`.

Runs the configured in-process LLM agent to generate a draft email from free-form user thoughts.

Current active implementation:

- uses `init_llm_agent(...)`
- provider is hardcoded to `llm::providers::Providers::Ollama`
- model comes from `config.default_model`
- API key comes from `config.api_key`
- `SearchEmailsTool` is registered as a tool

### `rater`

Used by `src/mail-app/init.js`.

Builds an `rpc_llm::Email` payload and sends it to the external RPC worker for spam/phishing classification.

---

## 10. LLM and RPC behavior that is currently active

## 10.1 Spam/phishing rating

### Files

- `src-tauri/src/rpc_llm/mod.rs`
- `src-tauri/src/rpc_llm/rpc/call.rs`

### Current active path

`rater(...)` uses `caller(...)`, which:

- sends a request over `tarpc`
- sets a 120-second deadline
- retries connection-style failures up to 3 times
- retries some inference crashes such as `MODEL_OOM`
- parses the returned JSON into `SpamRating`

`SpamRating` returns:

- `label`
  - `Ham`
  - `Spam`
  - `Phishing`
- `score`

## 10.2 Draft generation

Draft generation is separate from spam rating.

It does **not** use the RPC path above.

Instead, the active backend builds an LLM agent in-process during initialization and `email_generator(...)` calls:

- `initialized.llm_agent.agent().schema_chat(&thoughts).await`

So the current backend has **two distinct AI paths**:

1. **RPC path** for spam/phishing rating
2. **in-process Ollama-backed agent path** for draft generation

## 10.3 What is not active today

Although the repository contains local Python inference support and a local-inference provider in the `llm` crate, the active Tauri backend documented here does **not** invoke that path.

---

## 11. Logout and cleanup

### `src-tauri/src/handlers/menu.rs::logout_with_state`

Current logout flow:

1. send `ImapCommand::Logout` to the worker
2. cancel `logout_token`
3. cancel `refresh_token`
4. remove `config.json`
5. wait for logout confirmation from the worker over the logout channel
6. delete the local `data` directory
7. reset `AppState` to `Fresh`

### Worker-side logout behavior

On `ImapCommand::Logout`, the worker:

- logs out primary IMAP session if present
- logs out background IMAP session if present
- clears session/pool-related state
- sets `LOGGED_IN = false`
- sends logout completion

### Frontend effect

The menu logout path also emits:

- `app://logged-out`

The frontend listens for that event and redirects to `/login.html`.

---

## 12. Current execution flow summary

### Fresh install

1. app starts
2. `check_app_status` returns `Setup`
3. frontend opens `setup.html`
4. user submits IMAP server and port
5. `config_setup` writes config and initializes DB
6. frontend redirects to login page

### Normal login

1. user submits username/password
2. `login` initializes services
3. worker authenticates two IMAP sessions
4. backend stores password in Apple Keychain
5. backend sends initial Inbox fetch command
6. fetcher starts Inbox + concurrent Sent bootstrap
7. first page lands in SQLite
8. frontend receives initial-complete event and renders mail
9. user can paginate with `fetch_emails`
10. user can manually refresh with `refresh_emails_handler`

### Returning user

1. app starts
2. `check_app_status` finds config
3. backend retrieves password from Apple Keychain
4. worker authenticates IMAP in background
5. initialized state is restored
6. window is expanded and app enters signed-in mode

### Logout

1. user triggers menu logout
2. IMAP logout command is sent
3. cancellation tokens are cancelled
4. config and DB cache are removed
5. app emits logout event
6. frontend returns to login page

---

## 13. Current limitations and behavior notes

These are important if you want the docs to reflect reality rather than intent.

### Only `INBOX` and `Sent` are active backend mailboxes

- current sync code is built around those two mailboxes
- `Drafts` and `Trash` are not active backend sync targets

### Current app is effectively single-account

- config stores `accounts: Vec<String>`
- active runtime behavior uses the first account for keychain lookup and SMTP sender identity

### Auto-refresh scaffolding exists, but manual refresh is the active path

- the worker has a ticker-based refresh branch
- current code does not fully activate that branch
- frontend manual refresh is the dependable current behavior

### Python local inference is not part of the active Tauri flow

- present in repo
- not used by the currently wired backend commands

### macOS matters for stored credentials

- Apple Keychain integration is the active secure-storage mechanism
- the code explicitly documents non-macOS support as not implemented in the same way

---

## 14. File map for the active backend

### Core runtime

- `src-tauri/src/main.rs`
- `src-tauri/src/lib.rs`
- `src-tauri/src/tracing.rs`

### Config and startup

- `src-tauri/src/config.rs`
- `src-tauri/src/handlers/configuration.rs`
- `src-tauri/src/handlers/login.rs`

### IMAP and sync

- `src-tauri/src/imap.rs`
- `src-tauri/src/workers/mod.rs`
- `src-tauri/src/workers/imap_session.rs`
- `src-tauri/src/email_cache/fetcher.rs`
- `src-tauri/src/email_cache/mod.rs`

### Database

- `src-tauri/src/db/mod.rs`
- `src-tauri/src/db/emails_db.rs`

### Frontend command handlers

- `src-tauri/src/handlers/email.rs`
- `src-tauri/src/handlers/events.rs`
- `src-tauri/src/handlers/menu.rs`
- `src-tauri/src/handlers/llm.rs`

### AI/RPC

- `src-tauri/src/rpc_llm/mod.rs`
- `src-tauri/src/rpc_llm/rpc/call.rs`

---

## 15. Bottom line

The current RiiMail backend is an **async Tauri/Rust backend** built around:

- a Tokio runtime
- a long-lived IMAP worker task
- two authenticated IMAP sessions for safe concurrent mailbox work
- SQLite as the local mail cache
- SMTP for outbound mail
- an in-process LLM agent for drafting
- an external tarpc RPC service for spam/phishing scoring

Its concurrency model is not "lots of raw threads"; it is mostly:

- async tasks
- channel-based coordination
- shared session locks
- cancellation tokens
- event/oneshot signaling to the frontend

That is the current backend shape the app is actually using today.
