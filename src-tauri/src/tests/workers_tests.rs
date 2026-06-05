use crate::error::friendly_login_error_message;

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
