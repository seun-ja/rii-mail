use lettre::address::AddressError;

use serde::Serialize;
use tokio::sync::{mpsc::error::SendError, oneshot::error::RecvError};

use crate::{
    config::{ImapClientConfig, InitializedState},
    imap::ImapCommand,
};

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Serialization Error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("Error: {0}")]
    Other(String),
    #[error("Not Found")]
    NotFound,
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
    #[error("State Channel Send Error: {0}")]
    StateChannelSend(#[from] SendError<InitializedState>),
    #[error("Imap Config Channel Send Error: {0}")]
    ImapConfigChannelSend(#[from] SendError<ImapClientConfig>),
    #[error("Returning User Imap Config Channel Send Error: {0}")]
    ReturningUserImapConfigChannelSend(String),
    #[error("Imap Command Channel Send Error: {0}")]
    ImapCommandChannelSend(#[from] SendError<ImapCommand>),
    /// Authentication error: the provider returned an authentication error.
    #[error("authentication error: {0}")]
    Authentication(String),

    /// Keychain error: an error occurred while accessing the Apple keychain.
    #[error("keychain error: {0}")]
    #[cfg(target_os = "macos")]
    Keychain(#[from] security_framework::base::Error),
    /// Keychain error: Apple Keychain is only supported on macOS.
    #[error("keychain error: {0}")]
    #[cfg(not(target_os = "macos"))]
    Keychain(String),
    /// Byte conversion error: an error occurred while converting bytes to a string.
    #[error("byte conversion error: {0}")]
    ByteConversion(#[from] std::string::FromUtf8Error),
    #[error("Oneshot channel receive error: {0}")]
    OneShotRecv(#[from] RecvError),
    #[error("Thread cancellation fails")]
    ThreadCancel,
    #[error("SMTP Transport Relay Error: {0}")]
    SmtpTransportRelay(#[from] lettre::transport::smtp::Error),
    #[error("Error sending email: {0}")]
    SendEmail(#[from] lettre::error::Error),
    #[error("Error parsing Email Address")]
    AddressParse(#[from] AddressError),
    #[error("LLM Error")]
    Llm(#[from] llm::error::Error),
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
            Error::Io(e) => ("Io", e.to_string()),
            Error::Tls(e) => ("Tls", e.to_string()),
            Error::Imap(e) => ("Imap", e.to_string()),
            Error::Tauri(e) => ("Tauri", e.to_string()),
            Error::Sqlx(e) => ("SQLx", e.to_string()),
            Error::StateChannelSend(e) => ("StateChannelSend", e.to_string()),
            Error::ImapConfigChannelSend(e) => ("ImapConfigChannelSend", e.to_string()),
            Error::ReturningUserImapConfigChannelSend(e) => {
                ("ReturningUserImapConfigChannelSend", e.to_string())
            }
            Error::ImapCommandChannelSend(e) => ("ImapCommandChannelSend", e.to_string()),
            Error::Authentication(e) => ("Authentication", e.to_string()),

            Error::Keychain(e) => ("Keychain", e.to_string()),
            Error::ByteConversion(e) => ("ByteConversion", e.to_string()),
            Error::OneShotRecv(e) => ("OneShotRecv", e.to_string()),
            Error::ThreadCancel => ("ThreadCancel", self.to_string()),
            Error::SmtpTransportRelay(e) => ("SmtpTransportRelay", e.to_string()),
            Error::SendEmail(e) => ("SendEmail", e.to_string()),
            Error::AddressParse(e) => ("AddressParse", e.to_string()),
            Error::Llm(e) => ("LLM", e.to_string()),
        };

        use serde::ser::SerializeStruct;

        let mut state = serializer.serialize_struct("Error", 2)?;
        state.serialize_field("kind", kind)?;
        state.serialize_field("message", &message)?;
        state.end()
    }
}

pub type ErrorMessage = String;

pub(crate) fn friendly_login_error_message(stage: &str, raw: &str) -> String {
    let lower = raw.to_ascii_lowercase();

    if lower.contains("authentication")
        || lower.contains("invalid credentials")
        || lower.contains("login failed")
        || lower.contains("auth")
    {
        return "Invalid email or password. Please verify your credentials and try again."
            .to_string();
    }

    if lower.contains("timed out")
        || lower.contains("timeout")
        || lower.contains("connection")
        || lower.contains("dns")
        || lower.contains("network")
        || lower.contains("refused")
    {
        return "Unable to reach the mail server. Check your internet connection and IMAP server settings."
            .to_string();
    }

    if lower.contains("tls") || lower.contains("certificate") || lower.contains("ssl") {
        return "Secure connection to the mail server failed. Please verify TLS/SSL settings."
            .to_string();
    }

    match stage {
        "init" => {
            "Could not connect to the IMAP server. Please check server host and port.".to_string()
        }
        "login" => "Login failed due to a server error. Please try again shortly.".to_string(),
        _ => "Login failed. Please try again.".to_string(),
    }
}
