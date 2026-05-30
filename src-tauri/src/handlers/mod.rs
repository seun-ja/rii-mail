pub(crate) mod configuration;
pub(crate) mod email;
mod menu;
mod rater;

pub use configuration::{check_app_status, config_setup, login, open_main_window};
pub use email::{fetch_emails_handler, refresh_emails_handler};
pub use menu::logout_with_state;
pub use rater::rater;
