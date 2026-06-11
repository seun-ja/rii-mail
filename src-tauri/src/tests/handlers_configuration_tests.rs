use crate::{db::Provider, handlers::provider_from_imap_server};

#[test]
fn provider_from_imap_server_maps_yahoo_case_insensitively() {
    assert!(matches!(
        provider_from_imap_server("imap.mail.yahoo.com"),
        Provider::Yahoo
    ));

    assert!(matches!(
        provider_from_imap_server("IMAP.MAIL.YAHOO.COM"),
        Provider::Yahoo
    ));
}

#[test]
fn provider_from_imap_server_defaults_to_gmail_for_non_yahoo() {
    assert!(matches!(
        provider_from_imap_server("imap.gmail.com"),
        Provider::Gmail
    ));

    assert!(matches!(
        provider_from_imap_server("imap.example.com"),
        Provider::Gmail
    ));
}
