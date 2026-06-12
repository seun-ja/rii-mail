pub(crate) mod configuration;
pub(crate) mod email;
mod events;
mod llm;
mod login;
mod menu;

pub use configuration::{check_app_status, config_setup, open_main_window};
pub use email::{fetch_emails, refresh_emails_handler, send_email};
pub use events::{
    inbox_email_populated, inbox_intial_email_populated, sent_email_populated,
    sent_intial_email_populated,
};
use lettre::Address;
pub use llm::{email_generator, rater};
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
    } else {
        db::Provider::Gmail
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
