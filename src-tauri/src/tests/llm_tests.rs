use crate::llm::{EmailRequest, Label, SpamRating, _Providers};

#[test]
fn providers_display_and_parse_roundtrip() {
    assert_eq!(_Providers::from("ollama").to_string(), "ollama");
    assert_eq!(_Providers::from("OpenAI").to_string(), "openai");
    assert_eq!(_Providers::from("sagemaker").to_string(), "sagemaker");
    assert_eq!(_Providers::from("LOCAL").to_string(), "local");
}

#[test]
fn providers_from_panics_on_unknown_value() {
    let result = std::panic::catch_unwind(|| _Providers::from("unknown-provider"));
    assert!(result.is_err());
}

#[test]
fn email_request_display_formats_fields() {
    let req = EmailRequest {
        from: "alice@example.com".to_string(),
        subject: "Subject".to_string(),
        body: "Body".to_string(),
    };

    let rendered = req.to_string();

    assert!(rendered.contains("From: alice@example.com"));
    assert!(rendered.contains("Subject: Subject"));
    assert!(rendered.contains("Body: Body"));
}

#[test]
fn spam_rating_try_from_accepts_object_or_array() {
    let single = r#"{"label":"LABEL_1","score":0.98}"#.to_string();
    let from_single = SpamRating::try_from(single).expect("single object should parse");
    assert_eq!(from_single.label, Label::Spam);

    let vec_payload = r#"[{"label":"LABEL_2","score":0.75}]"#.to_string();
    let from_vec = SpamRating::try_from(vec_payload).expect("array payload should parse");
    assert_eq!(from_vec.label, Label::Phishing);
}

#[test]
fn spam_rating_try_from_invalid_json_errors() {
    let err = SpamRating::try_from("not-json".to_string())
        .expect_err("invalid payload should fail parsing");

    assert!(err.to_string().contains("Serialization Error"));
}

#[test]
fn label_serializes_to_title_case() {
    let ham = serde_json::to_string(&Label::Ham).expect("label should serialize");
    let spam = serde_json::to_string(&Label::Spam).expect("label should serialize");
    let phishing = serde_json::to_string(&Label::Phishing).expect("label should serialize");

    assert_eq!(ham, "\"Ham\"");
    assert_eq!(spam, "\"Spam\"");
    assert_eq!(phishing, "\"Phishing\"");
}
