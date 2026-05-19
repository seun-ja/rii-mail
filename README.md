# Email Desktop Client

A Tauri-based desktop application for analyzing and rating emails for spam/phishing content. The app integrates with an RPC-based AI agent service and IMAP for email management.

## Features

- **Email Spam Rating**: Analyze email subject, sender, and body to determine spam/phishing likelihood
- **Setup Wizard**: First-run configuration for IMAP and RPC server settings
- **Dynamic Initialization**: Backend services initialize on demand after configuration
- **Configuration Management**: File-based config with environment variable defaults
- **Secure IMAP Connection**: TLS-encrypted IMAP sessions for email access

## Project Structure

```
src/                    # Frontend (HTML, CSS, JavaScript)
├── index.html         # Main app page (email input)
├── setup.html         # Setup/configuration page
├── main.js            # Main app logic and email rating
├── setup.js           # Setup form handling
└── styles.css         # Shared styling

src-tauri/             # Rust backend
├── src/
│   ├── lib.rs         # Main entry point and Tauri command handlers
│   ├── config.rs      # Configuration management and state
│   ├── error.rs       # Error types and serialization
│   ├── handlers/      # Tauri command handlers
│   ├── rpc/           # RPC agent client and communication
│   ├── session/       # IMAP session management
│   └── tracing.rs     # Logging configuration
└── Cargo.toml         # Rust dependencies
```

## Getting Started

### Prerequisites

- Node.js 16+ and npm/yarn
- Rust 1.70+ (install via [rustup](https://rustup.rs/))
- Tauri CLI (`cargo install tauri-cli`)

### Installation

1. **Clone and install dependencies:**
   ```bash
   git clone https://github.com/seun-ja/email-desktop-client.git
   cd Pemail
   npm install
   ```

2. **Create `.env` file** with configuration defaults:
   ```bash
   VITE_RPC_SERVER=0.0.0.0:5500
   VITE_RUST_LOG=info
   VITE_OTLP_COLLECTOR_ENDPOINT=http://otel-collector:4317
   ```

3. **Build and run the app:**
   ```bash
   npm run tauri dev
   ```

## Configuration

On first run, the app will redirect to `/setup.html` to configure:
- **IMAP Server**: Email server hostname (default: `imap.gmail.com`)
- **IMAP Port**: Email server port (default: `993`)
- Other defaults (RPC Server, logging, OTLP) are sourced from `.env`

Configuration is saved to `~/.config/Pemail/config.json` and persists across sessions.

## Usage

1. **Setup Phase**: Fill in IMAP credentials and settings → Click "Save Setup" → App reloads
2. **Main App**: 
   - Enter email subject, sender email, and body
   - Click "Test Input" to send to the RPC agent for spam rating
   - View results: Spam rating label (Ham/Spam/Phishing) and confidence score

## Architecture

### Frontend
- Vanilla JavaScript with Tauri IPC for command invocation
- Setup page handles first-run configuration
- Main page submits emails and displays spam ratings

### Backend (Rust + Tauri)
- **AppState**: RwLock-wrapped optional initialized state (async-safe)
- **Config**: File-based (JSON) with environment variable fallbacks
- **Tauri Commands**:
  - `config_setup(config)`: Save config and initialize services
  - `is_initialized()`: Check if backend services are ready
  - `rater(subject, emailFrom, body)`: Rate email for spam
- **RPC Client**: Async tarpc-based communication with ML agent service
- **IMAP Client**: async_imap for email session management

## Development

### Recommended IDE Setup

- [VS Code](https://code.visualstudio.com/)
- [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) extension
- [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)

### Build Commands

```bash
npm run tauri dev       # Development with hot-reload
npm run tauri build     # Production binary
npm run tauri build --target universal-apple-darwin  # Universal macOS
```

## Troubleshooting

- **Setup doesn't trigger redirect**: Check that `is_initialized` is called after `config_setup` completes; app reloads to pick up new state.
- **RPC connection fails**: Ensure RPC server is running at `VITE_RPC_SERVER` address.
- **IMAP connection fails**: Verify IMAP credentials and port in config; some providers require app-specific passwords.

## License

MIT
