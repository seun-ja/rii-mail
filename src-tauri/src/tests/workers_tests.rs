use crate::error::{friendly_login_error_message, Error};
use crate::workers::{is_imap_connection_error, FetchManager};

#[tokio::test]
async fn starting_a_session_replaces_cancelled_fetch_tokens() {
    let manager = FetchManager::default();
    manager.logout_token.lock().await.cancel();
    manager.refresh_token.lock().await.cancel();

    manager.reset_for_session().await;

    assert!(!manager.logout_token.lock().await.is_cancelled());
    assert!(!manager.refresh_token.lock().await.is_cancelled());
}

#[test]
fn login_error_message_maps_auth_failures() {
    let msg = friendly_login_error_message("login", "authentication failed");
    assert!(msg.contains("Invalid email or password"));
}

#[test]
fn login_error_message_maps_network_failures() {
    let msg = friendly_login_error_message("init", "connection timed out");
    assert!(msg.contains("Unable to reach the mail server"));
}

#[test]
fn login_error_message_maps_tls_failures() {
    let msg = friendly_login_error_message("login", "tls handshake failed");
    assert!(msg.contains("Secure connection"));
}

#[test]
fn login_error_message_uses_stage_default_for_init() {
    let msg = friendly_login_error_message("init", "unexpected");
    assert!(msg.contains("Could not connect to the IMAP server"));
}

#[test]
fn imap_io_error_is_treated_as_a_connection_failure() {
    let io_error = std::io::Error::new(
        std::io::ErrorKind::ConnectionReset,
        "connection closed via error",
    );
    let error = Error::Imap(async_imap::error::Error::Io(io_error));

    assert!(is_imap_connection_error(&error));
}

#[test]
fn imap_connection_lost_is_treated_as_a_connection_failure() {
    let error = Error::Imap(async_imap::error::Error::ConnectionLost);

    assert!(is_imap_connection_error(&error));
}

#[test]
fn imap_protocol_error_is_not_treated_as_a_connection_failure() {
    let error = Error::Imap(async_imap::error::Error::Bad("invalid command".to_string()));

    assert!(!is_imap_connection_error(&error));
}
