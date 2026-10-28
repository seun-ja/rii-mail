use crate::{
    db::Provider,
    handlers::{provider_from_imap_server, resolve_smtp_relay_url, smtp_relay_candidates},
};

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
fn provider_from_imap_server_defaults_to_custom_imap_server_for_non_yahoo() {
    assert!(matches!(
        provider_from_imap_server("imap.gmail.com"),
        Provider::Gmail
    ));

    assert!(matches!(
        provider_from_imap_server("imap.example.com"),
        Provider::CustomImapServer
    ));
}

#[tokio::test]
async fn smtp_relay_uses_explicit_configuration() {
    let relay = resolve_smtp_relay_url("mail.ebsafr.com", Some(" smtp.ebsafr.com "))
        .await
        .expect("an explicitly configured SMTP relay should be accepted");

    assert_eq!(relay, "smtp.ebsafr.com");
}

#[test]
fn smtp_relay_candidates_include_known_provider_endpoints() {
    let cases = [
        ("imap.mail.yahoo.com", "smtp.mail.yahoo.com"),
        ("IMAP.GMAIL.COM.", "smtp.gmail.com"),
        ("outlook.office365.com", "smtp.office365.com"),
        ("imap-mail.outlook.com", "smtp-mail.outlook.com"),
    ];

    for (imap_server, expected_smtp_relay) in cases {
        assert_eq!(
            smtp_relay_candidates(imap_server)
                .first()
                .map(String::as_str),
            Some(expected_smtp_relay)
        );
    }
}

#[test]
fn smtp_relay_candidates_cover_common_custom_server_conventions() {
    assert_eq!(
        smtp_relay_candidates("mail.ebsafr.com"),
        vec!["mail.ebsafr.com", "smtp.ebsafr.com"]
    );
    assert_eq!(
        smtp_relay_candidates("imap.example.com"),
        vec!["smtp.example.com", "mail.example.com"]
    );
    assert_eq!(
        smtp_relay_candidates("imap-mail.example.com"),
        vec!["smtp-mail.example.com"]
    );
}

#[tokio::test]
async fn smtp_relay_returns_discovery_error_when_no_candidate_can_be_generated() {
    let error = resolve_smtp_relay_url("mx.internal.example", None)
        .await
        .expect_err("setup should request an explicit SMTP server");

    assert!(matches!(error, crate::error::Error::SmtpDiscovery(_)));
}

#[tokio::test]
async fn smtp_relay_rejects_empty_explicit_configuration() {
    let error = resolve_smtp_relay_url("imap.gmail.com", Some("  "))
        .await
        .expect_err("an empty SMTP server should be treated as invalid configuration");

    assert!(error.to_string().contains("SMTP relay server is empty"));
}
