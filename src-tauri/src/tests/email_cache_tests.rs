use crate::email_cache::{map_folder_and_starred, normalize_text, CompleteEmail, EmailContent};

#[test]
fn normalize_text_compacts_whitespace() {
    let value = "  hello\n\tworld   from   riimail  ";
    assert_eq!(normalize_text(value, 100), "hello world from riimail");
}

#[test]
fn normalize_text_truncates_and_appends_ellipsis() {
    let value = "abcdefghijklmnopqrstuvwxyz";
    assert_eq!(normalize_text(value, 10), "abcdefghij...");
}

#[test]
fn map_folder_and_starred_prioritizes_sent_and_starred() {
    let labels = vec![
        "\\Inbox".to_string(),
        "\\Sent".to_string(),
        "Starred".to_string(),
    ];
    let (folder, starred) = map_folder_and_starred(&labels);

    assert_eq!(folder, "sent");
    assert!(starred);
}

#[test]
fn map_folder_and_starred_maps_spam_to_archive() {
    let labels = vec!["Spam".to_string()];
    let (folder, starred) = map_folder_and_starred(&labels);

    assert_eq!(folder, "archive");
    assert!(!starred);
}

#[test]
fn into_frontend_uses_text_body_when_available() {
    let complete = CompleteEmail {
        date: None,
        labels: Some(vec!["\\Inbox".to_string()]),
        body: Some(EmailContent {
            subject: " Subject ".to_string(),
            sender_name: "Alice".to_string(),
            email_from: "alice@example.com".to_string(),
            preview: "Preview   text".to_string(),
            html_body: "<p>fallback</p>".to_string(),
            text_body: " Hello  world ".to_string(),
            attachments: vec![],
            parts: vec![],
            raw_message: "raw".to_string(),
        }),
    };

    let frontend = complete.into_frontend("id-1".to_string());

    assert_eq!(frontend.id, "id-1");
    assert_eq!(frontend.folder, "inbox");
    assert_eq!(frontend.subject, "Subject");
    assert_eq!(frontend.preview, "Preview text");
    assert_eq!(frontend.body, "Hello world");
    assert_eq!(frontend.text_body, " Hello  world ");
    assert_eq!(frontend.html_body, "<p>fallback</p>");
    assert!(!frontend.starred);
    assert_eq!(frontend.time, "Unknown");
}

#[test]
fn into_frontend_falls_back_when_body_is_missing() {
    let complete = CompleteEmail {
        date: None,
        labels: None,
        body: None,
    };

    let frontend = complete.into_frontend("id-2".to_string());

    assert_eq!(frontend.id, "id-2");
    assert_eq!(frontend.folder, "inbox");
    assert_eq!(frontend.subject, "");
    assert_eq!(frontend.preview, "");
    assert_eq!(frontend.body, "");
    assert_eq!(frontend.time, "Unknown");
}
