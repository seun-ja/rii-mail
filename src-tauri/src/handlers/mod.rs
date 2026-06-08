pub(crate) mod configuration;
pub(crate) mod email;
mod events;
mod login;
mod menu;
mod rater;

pub use configuration::{check_app_status, config_setup, open_main_window};
pub use email::{fetch_emails_handler, refresh_emails_handler};
pub use events::{inbox_email_populated, sent_email_populated};
pub use login::login;
pub use menu::logout_with_state;
pub use rater::rater;

use crate::db::{self, Providers};

pub(crate) fn provider_from_imap_server(imap_server: &str) -> Providers {
    if imap_server.to_ascii_lowercase().contains("yahoo") {
        db::Providers::Yahoo
    } else {
        db::Providers::Gmail
    }
}
