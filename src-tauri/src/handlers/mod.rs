pub(crate) mod configuration;
pub(crate) mod email;
mod events;
mod llm;
mod llm_connections;
mod login;
mod menu;

pub use configuration::{
    check_app_status, config_setup, enable_main_window_controls, finish_startup, open_main_window,
};
#[cfg(test)]
pub(crate) use configuration::{resolve_smtp_relay_url, smtp_relay_candidates};
pub use email::{fetch_emails, refresh_emails_handler, send_email};
pub use events::{
    inbox_email_populated, inbox_intial_email_populated, sent_email_populated,
    sent_intial_email_populated,
};
use lettre::Address;
pub use llm::{email_generator, is_llm_enabled, rater};
pub use llm_connections::{
    connect_chatgpt, connect_ollama, ensure_chatgpt_agent_fresh, refresh_chatgpt_token_if_needed,
};
pub use login::login;
pub use menu::logout_with_state;

use crate::{
    config::Account,
    db::{self, Provider},
    error::Error,
};

pub(crate) fn provider_from_imap_server(imap_server: &str) -> Provider {
    if imap_server.to_ascii_lowercase().contains("yahoo") {
        db::Provider::Yahoo
    } else if imap_server.to_ascii_lowercase().contains("gmail") {
        db::Provider::Gmail
    } else if imap_server.to_ascii_lowercase().contains("outlook") {
        db::Provider::Outlook
    } else {
        db::Provider::CustomImapServer
    }
}

#[derive(Debug, Clone, PartialOrd, Ord, PartialEq, Eq, Hash)]
pub struct MailAddress {
    /// The name associated with the address.
    name: Option<String>,

    /// The email address itself.
    email: Address,
}

impl TryFrom<(Option<String>, String)> for MailAddress {
    type Error = Error;
    fn try_from(value: (Option<String>, String)) -> Result<Self, Self::Error> {
        let s = Self {
            name: value.0,
            email: value.1.parse()?,
        };

        Ok(s)
    }
}

impl TryFrom<Account> for MailAddress {
    type Error = Error;
    fn try_from(value: Account) -> Result<Self, Self::Error> {
        let s = Self {
            name: value.name,
            email: value.email.parse()?,
        };

        Ok(s)
    }
}

impl From<MailAddress> for lettre::message::Mailbox {
    fn from(value: MailAddress) -> Self {
        Self {
            name: value.name,
            email: value.email,
        }
    }
}
