use std::{error::Error as StdError, io::ErrorKind};

use serde::{Deserialize, Serialize};

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Serialization Error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("Error: {0}")]
    Other(String),
    #[error("Not Found")]
    NotFound,
    #[error("RPC Server Error: {0}")]
    RPCServer(#[from] rpc_agent::error::ApiError),
    #[error("IO Error: {0}")]
    Io(#[from] std::io::Error),
    #[error("TLS Error: {0}")]
    Tls(#[from] async_native_tls::Error),
    #[error("IMAP Error: {0}")]
    Imap(#[from] async_imap::error::Error),
    #[error("Tauri Error: {0}")]
    Tauri(#[from] tauri::Error),
    #[error("SQLx Error: {0}")]
    Sqlx(#[from] sqlx::Error),
}

impl Serialize for Error {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let (kind, message) = match self {
            Error::Serialization(e) => ("Serialization", e.to_string()),
            Error::Other(msg) => ("Other", msg.clone()),
            Error::NotFound => ("NotFound", self.to_string()),
            Error::RPCServer(e) => ("RPCServer", e.to_string()),
            Error::Io(e) => ("Io", e.to_string()),
            Error::Tls(e) => ("Tls", e.to_string()),
            Error::Imap(e) => ("Imap", e.to_string()),
            Error::Tauri(e) => ("Tauri", e.to_string()),
            Error::Sqlx(e) => ("SQLx", e.to_string()),
        };

        use serde::ser::SerializeStruct;

        let mut state = serializer.serialize_struct("Error", 2)?;
        state.serialize_field("kind", kind)?;
        state.serialize_field("message", &message)?;
        state.end()
    }
}

pub enum CrashAction {
    Retry { backoff_ms: u64, reason: String },
    Fatal,
    Ignore,
}

#[derive(Debug, Deserialize)]
struct RetryContext<'a> {
    message: &'a str,
    reason: &'a str,
}

pub fn inference_crash_handler(err: &str, attempt: u32) -> CrashAction {
    if let Ok(err_json) = serde_json::from_str::<RetryContext>(err) {
        match err_json.message {
            "MODEL_OOM" => {
                let backoff = 100 * (attempt + 1) as u64;
                return CrashAction::Retry {
                    backoff_ms: backoff,
                    reason: err_json.reason.to_string(),
                };
            }
            "MODEL_FATAL" => {
                return CrashAction::Fatal;
            }
            _ => {}
        }
    }
    CrashAction::Ignore
}

/// Returns true if the error or any of its sources is a connection error (BrokenPipe, ConnectionReset, ConnectionAborted, NotConnected)
pub fn is_connection_error(e: &(dyn StdError + 'static)) -> bool {
    if let Some(io_err) = e.downcast_ref::<std::io::Error>() {
        matches!(
            io_err.kind(),
            ErrorKind::BrokenPipe
                | ErrorKind::ConnectionReset
                | ErrorKind::ConnectionAborted
                | ErrorKind::NotConnected
        )
    } else if let Some(source) = e.source() {
        is_connection_error(source)
    } else {
        false
    }
}
