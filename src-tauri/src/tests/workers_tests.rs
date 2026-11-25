use crate::error::{friendly_login_error_message, Error};
use crate::{
    db::{init_db, Provider},
    imap::ImapCommand,
    workers::{
        cleanup_logout_resources, is_imap_connection_error, queue_initial_fetch, FetchManager,
    },
};
use std::time::{SystemTime, UNIX_EPOCH};

fn unique_test_dir(prefix: &str) -> std::path::PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be monotonic for tests")
        .as_nanos();
    std::env::temp_dir().join(format!("riimail-{prefix}-{nanos}"))
}

#[tokio::test]
async fn logout_cleanup_closes_pool_and_removes_cached_data() {
    let app_dir = unique_test_dir("logout-cleanup");
    let data_path = app_dir.join("data");
    let pool = init_db(&app_dir, "emails.db")
        .await
        .expect("database should initialize");
    assert!(data_path.exists());

    cleanup_logout_resources(Some(pool), Some(data_path.clone()), None, None).await;

    assert!(!data_path.exists());
    let _ = std::fs::remove_dir_all(app_dir);
}

#[tokio::test]
async fn starting_a_session_replaces_cancelled_fetch_tokens() {
    let manager = FetchManager::default();
    manager.logout_token.lock().await.cancel();
    manager.refresh_token.lock().await.cancel();

    manager.reset_for_session().await;

    assert!(!manager.logout_token.lock().await.is_cancelled());
    assert!(!manager.refresh_token.lock().await.is_cancelled());
}

#[tokio::test]
async fn authenticated_session_queues_initial_mailbox_import() {
    let manager = FetchManager::default();
    manager.logout_token.lock().await.cancel();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

    queue_initial_fetch(&manager, &tx, Provider::Yahoo)
        .await
        .expect("initial fetch should be queued");

    match rx.recv().await.expect("initial fetch command") {
        ImapCommand::FetchEmails(provider, token) => {
            assert_eq!(provider, Provider::Yahoo);
            assert!(!token.is_cancelled());
        }
        command => panic!("expected initial fetch command, got {command:?}"),
    }
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
