use std::time::Duration;

use crate::{auth::login, error::Error};

#[tokio::test]
async fn login_without_client_returns_expected_error_immediately() {
    let result = tokio::time::timeout(
        Duration::from_millis(100),
        login("user@example.com", "not-used", None),
    )
    .await
    .expect("a missing client should return without attempting network access");

    match result {
        Err(Error::Other(message)) => {
            assert_eq!(message, "Failed to Initiate TlsStream Client");
        }
        Err(error) => panic!("unexpected error variant: {error}"),
        Ok(_) => panic!("login without a client unexpectedly succeeded"),
    }
}
