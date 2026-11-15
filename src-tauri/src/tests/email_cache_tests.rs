use crate::email_cache::{
    encode_cached_email_content, map_folder_and_starred, normalize_text, CompleteEmail,
    EmailAttachment, EmailContent, StandardEmail,
};

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
        uid: Some(1),
        date: None,
        labels: Some(vec!["\\Inbox".to_string()]),
        read: true,
        body: Some(EmailContent {
            subject: " Subject ".to_string(),
            sender_name: "Alice".to_string(),
            email_from: "alice@example.com".to_string(),
            cc: Some(vec![("Bob".to_string(), "bob@example.com".to_string())]),
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
    assert_eq!(frontend.uid, Some(1));
    assert_eq!(frontend.folder, "inbox");
    assert_eq!(frontend.subject, "Subject");
    assert_eq!(frontend.preview, "Preview text");
    assert_eq!(frontend.body, "Hello world");
    assert_eq!(frontend.text_body, " Hello  world ");
    assert_eq!(frontend.html_body, "<p>fallback</p>");
    assert_eq!(
        frontend.cc,
        Some(vec![("Bob".to_string(), "bob@example.com".to_string())])
    );
    assert!(!frontend.starred);
    assert!(frontend.read);
    assert_eq!(frontend.time, "Unknown");
}

#[test]
fn structured_email_cache_roundtrips_attachment_metadata_without_bytes() {
    let content = EmailContent::from_header_and_parts(
        b"From: Alice <alice@example.com>\r\nSubject: Cached\r\n",
        "Hello".to_string(),
        String::new(),
        vec![EmailAttachment {
            id: 0,
            part_id: "2.1".to_string(),
            file_name: "report.pdf".to_string(),
            content_type: "application/pdf".to_string(),
            size: 4096,
            data: Vec::new(),
        }],
    );
    let encoded = encode_cached_email_content(&content).expect("content should serialize");
    assert!(!String::from_utf8_lossy(&encoded).contains("attachment bytes"));

    let complete = CompleteEmail::from(StandardEmail {
        uid: Some(8),
        date: None,
        body: Some(encoded),
        labels: None,
        read: false,
    });
    let cached = complete.body.expect("structured content should decode");

    assert_eq!(cached.subject, "Cached");
    assert_eq!(cached.text_body, "Hello");
    assert_eq!(cached.attachments[0].part_id, "2.1");
    assert!(cached.attachments[0].data.is_empty());
}

#[test]
fn email_content_extracts_attachment_metadata_and_decoded_bytes() {
    let raw_message = concat!(
        "From: Alice <alice@example.com>\r\n",
        "Subject: Attachment test\r\n",
        "Content-Type: multipart/mixed; boundary=boundary\r\n",
        "\r\n",
        "--boundary\r\n",
        "Content-Type: text/plain; charset=utf-8\r\n",
        "\r\n",
        "See attachment.\r\n",
        "--boundary\r\n",
        "Content-Type: application/pdf; name=report.pdf\r\n",
        "Content-Disposition: attachment; filename=report.pdf\r\n",
        "Content-Transfer-Encoding: base64\r\n",
        "\r\n",
        "AQIDBA==\r\n",
        "--boundary--\r\n",
    );
    let complete = CompleteEmail::from(StandardEmail {
        uid: Some(4),
        date: None,
        body: Some(raw_message.as_bytes().to_vec()),
        labels: None,
        read: false,
    });

    let content = complete.body.expect("message should parse");
    assert_eq!(content.attachments.len(), 1);

    let attachment = &content.attachments[0];
    assert_eq!(attachment.id, 0);
    assert_eq!(attachment.part_id, "1");
    assert_eq!(attachment.file_name, "report.pdf");
    assert_eq!(attachment.content_type, "application/pdf");
    assert_eq!(attachment.size, 4);
    assert_eq!(attachment.data, vec![1, 2, 3, 4]);
}

#[test]
fn email_content_removes_path_components_from_attachment_names() {
    let raw_message = concat!(
        "From: alice@example.com\r\n",
        "Content-Type: multipart/mixed; boundary=boundary\r\n",
        "\r\n",
        "--boundary\r\n",
        "Content-Type: application/octet-stream\r\n",
        "Content-Disposition: attachment; filename=../../unsafe.txt\r\n",
        "\r\n",
        "contents\r\n",
        "--boundary--\r\n",
    );
    let complete = CompleteEmail::from(StandardEmail {
        uid: Some(5),
        date: None,
        body: Some(raw_message.as_bytes().to_vec()),
        labels: None,
        read: false,
    });

    let content = complete.body.expect("message should parse");
    assert_eq!(content.attachments[0].file_name, "unsafe.txt");
}

#[test]
fn email_content_extracts_all_cc_addresses() {
    let raw_message = concat!(
        "From: Alice <alice@example.com>\r\n",
        "Cc: Bob <bob@example.com>, carol@example.com\r\n",
        "Subject: CC test\r\n",
        "Content-Type: text/plain; charset=utf-8\r\n",
        "\r\n",
        "Hello"
    );
    let complete = CompleteEmail::from(StandardEmail {
        uid: Some(2),
        date: None,
        body: Some(raw_message.as_bytes().to_vec()),
        labels: None,
        read: false,
    });

    let content = complete.body.expect("message should parse");

    assert_eq!(
        content.cc,
        Some(vec![
            ("Bob".to_string(), "bob@example.com".to_string()),
            (String::new(), "carol@example.com".to_string()),
        ])
    );
}

#[test]
fn email_content_flattens_grouped_cc_addresses() {
    let raw_message = concat!(
        "From: alice@example.com\r\n",
        "Cc: Engineering: Bob <bob@example.com>, Carol <carol@example.com>;\r\n",
        "Subject: Group CC test\r\n",
        "Content-Type: text/plain; charset=utf-8\r\n",
        "\r\n",
        "Hello"
    );
    let complete = CompleteEmail::from(StandardEmail {
        uid: Some(3),
        date: None,
        body: Some(raw_message.as_bytes().to_vec()),
        labels: None,
        read: false,
    });

    let content = complete.body.expect("message should parse");

    assert_eq!(
        content.cc,
        Some(vec![
            ("Bob".to_string(), "bob@example.com".to_string()),
            ("Carol".to_string(), "carol@example.com".to_string()),
        ])
    );
}

#[test]
fn into_frontend_falls_back_when_body_is_missing() {
    let complete = CompleteEmail {
        uid: None,
        date: None,
        labels: None,
        body: None,
        read: false,
    };

    let frontend = complete.into_frontend("id-2".to_string());

    assert_eq!(frontend.id, "id-2");
    assert_eq!(frontend.folder, "inbox");
    assert_eq!(frontend.subject, "");
    assert_eq!(frontend.preview, "");
    assert_eq!(frontend.body, "");
    assert_eq!(frontend.time, "Unknown");
}
