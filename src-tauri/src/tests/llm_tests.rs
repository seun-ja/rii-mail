use std::time::{SystemTime, UNIX_EPOCH};

use crate::llm::{resolve_local_inference_paths, Email, Label, SpamRating};
use serde_json::json;

fn unique_test_dir(name: &str) -> std::path::PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be after the Unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("riimail-llm-paths-{name}-{nanos}"))
}

#[test]
fn email_display_uses_the_expected_format_without_escaping_content() {
    let email = Email {
        from: "Alice Example <alice@example.com>".to_string(),
        subject: "Status: ready".to_string(),
        body: "First line\nSecond line".to_string(),
    };

    assert_eq!(
        email.to_string(),
        "From: Alice Example <alice@example.com>\nSubject: Status: ready\nBody: First line\nSecond line"
    );
}

#[test]
fn email_serializes_with_all_fields() {
    let email = Email {
        from: "alice@example.com".to_string(),
        subject: "Hello".to_string(),
        body: "Body with a \"quote\" and a newline\n".to_string(),
    };

    let serialized = serde_json::to_value(&email).expect("email should serialize");

    assert_eq!(
        serialized,
        json!({
            "from": "alice@example.com",
            "subject": "Hello",
            "body": "Body with a \"quote\" and a newline\n",
        })
    );
}

#[test]
fn email_deserializes_with_all_fields() {
    let email: Email = serde_json::from_value(json!({
        "from": "sender@example.com",
        "subject": "A subject",
        "body": "A body",
    }))
    .expect("valid email should deserialize");

    assert_eq!(email.from, "sender@example.com");
    assert_eq!(email.subject, "A subject");
    assert_eq!(email.body, "A body");
}

#[test]
fn email_deserialization_rejects_missing_fields() {
    for payload in [
        json!({"subject": "Subject", "body": "Body"}),
        json!({"from": "alice@example.com", "body": "Body"}),
        json!({"from": "alice@example.com", "subject": "Subject"}),
    ] {
        assert!(
            serde_json::from_value::<Email>(payload).is_err(),
            "an email with a missing field should be rejected"
        );
    }
}

#[test]
fn spam_rating_try_from_accepts_an_object_and_preserves_score() {
    let rating = SpamRating::try_from(r#"{"label":"LABEL_1","score":0.625}"#.to_string())
        .expect("single object should parse");

    assert_eq!(rating.label, Label::Spam);
    assert_eq!(rating.score, 0.625);
}

#[test]
fn spam_rating_try_from_accepts_an_array_and_uses_the_first_rating() {
    let rating = SpamRating::try_from(
        r#"[{"label":"LABEL_2","score":0.75},{"label":"LABEL_0","score":0.25}]"#.to_string(),
    )
    .expect("array payload should parse");

    assert_eq!(rating.label, Label::Phishing);
    assert_eq!(rating.score, 0.75);
}

#[test]
fn spam_rating_try_from_empty_array_returns_error_without_panicking() {
    let result = std::panic::catch_unwind(|| SpamRating::try_from("[]".to_string()));

    let parse_result = result.expect("empty arrays must not panic");
    let error = parse_result.expect_err("empty arrays should be rejected");
    assert!(
        error
            .to_string()
            .contains("spam rating array cannot be empty"),
        "unexpected error: {error}"
    );
}

#[test]
fn spam_rating_try_from_rejects_malformed_json() {
    for payload in [
        "not-json",
        r#"{"label":"LABEL_1","score":0.5"#,
        r#"[{"label":"LABEL_1","score":0.5},]"#,
    ] {
        let error = SpamRating::try_from(payload.to_string())
            .expect_err("malformed JSON should fail parsing");

        assert!(
            error.to_string().contains("Serialization Error"),
            "unexpected error for {payload:?}: {error}"
        );
    }
}

#[test]
fn spam_rating_try_from_rejects_missing_fields() {
    for payload in [
        r#"{"score":0.5}"#,
        r#"{"label":"LABEL_0"}"#,
        r#"[{"score":0.5}]"#,
        r#"[{"label":"LABEL_0"}]"#,
    ] {
        assert!(
            SpamRating::try_from(payload.to_string()).is_err(),
            "payload with a missing field should fail: {payload}"
        );
    }
}

#[test]
fn spam_rating_try_from_rejects_unknown_labels_and_wrong_field_types() {
    for payload in [
        r#"{"label":"LABEL_99","score":0.5}"#,
        r#"{"label":"Spam","score":0.5}"#,
        r#"{"label":"LABEL_1","score":"high"}"#,
        r#"{"label":1,"score":0.5}"#,
    ] {
        assert!(
            SpamRating::try_from(payload.to_string()).is_err(),
            "invalid field value should fail: {payload}"
        );
    }
}

#[test]
fn spam_rating_try_from_ignores_unknown_fields() {
    let rating = SpamRating::try_from(
        r#"{"label":"LABEL_0","score":0.375,"explanation":"ordinary mail"}"#.to_string(),
    )
    .expect("unknown fields should be ignored by serde");

    assert_eq!(rating.label, Label::Ham);
    assert_eq!(rating.score, 0.375);
}

#[test]
fn spam_rating_serializes_as_an_object_with_title_case_label() {
    let rating = SpamRating {
        label: Label::Phishing,
        score: 0.875,
    };

    let serialized = serde_json::to_value(&rating).expect("spam rating should serialize");

    assert_eq!(
        serialized,
        json!({
            "label": "Phishing",
            "score": 0.875,
        })
    );
}

#[test]
fn labels_serialize_to_title_case_names() {
    for (label, expected) in [
        (Label::Ham, "Ham"),
        (Label::Spam, "Spam"),
        (Label::Phishing, "Phishing"),
    ] {
        assert_eq!(
            serde_json::to_string(&label).expect("label should serialize"),
            format!(r#""{expected}""#)
        );
    }
}

#[test]
fn labels_deserialize_from_model_label_names() {
    for (encoded, expected) in [
        (r#""LABEL_0""#, Label::Ham),
        (r#""LABEL_1""#, Label::Spam),
        (r#""LABEL_2""#, Label::Phishing),
    ] {
        let label: Label = serde_json::from_str(encoded).expect("known label should deserialize");
        assert_eq!(label, expected);
    }
}

#[test]
fn label_deserialization_rejects_unknown_or_serialized_names() {
    for encoded in [r#""LABEL_3""#, r#""Spam""#, r#""spam""#, "null", "1"] {
        assert!(
            serde_json::from_str::<Label>(encoded).is_err(),
            "unknown label should fail: {encoded}"
        );
    }
}

#[test]
fn app_error_preserves_the_underlying_llm_message() {
    let error = crate::error::Error::from(::llm::error::Error::Other(
        "bundled Python runtime is missing".to_string(),
    ));

    assert_eq!(
        error.to_string(),
        "LLM error: bundled Python runtime is missing"
    );
}

#[test]
fn local_inference_paths_resolve_existing_resources_to_absolute_paths() {
    let root = unique_test_dir("success");
    let model = root.join("local-model");
    let script = root.join("llm/local_inference.py");
    std::fs::create_dir_all(&model).expect("model directory should be created");
    std::fs::create_dir_all(script.parent().expect("script should have a parent"))
        .expect("script directory should be created");
    std::fs::write(&script, "def predict(_text): return {}\n").expect("script should be written");

    let (resolved_model, resolved_script) =
        resolve_local_inference_paths(&root).expect("paths should resolve");

    assert!(resolved_model.is_absolute());
    assert!(resolved_script.is_absolute());
    assert_eq!(
        resolved_model,
        model.canonicalize().expect("model should resolve")
    );
    assert_eq!(
        resolved_script,
        script.canonicalize().expect("script should resolve")
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn local_inference_paths_reject_missing_model_directory() {
    let root = unique_test_dir("missing-model");
    let script = root.join("llm/local_inference.py");
    std::fs::create_dir_all(script.parent().expect("script should have a parent"))
        .expect("script directory should be created");
    std::fs::write(script, "def predict(_text): return {}\n").expect("script should be written");

    let error = resolve_local_inference_paths(&root)
        .expect_err("a missing model directory should return an error");

    assert!(error
        .to_string()
        .contains("bundled local model is unavailable"));
    assert!(error.to_string().contains("local-model"));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn local_inference_paths_reject_missing_python_script() {
    let root = unique_test_dir("missing-script");
    std::fs::create_dir_all(root.join("local-model")).expect("model directory should be created");

    let error = resolve_local_inference_paths(&root)
        .expect_err("a missing Python script should return an error");

    assert!(error
        .to_string()
        .contains("bundled Python inference script is unavailable"));
    assert!(error.to_string().contains("local_inference.py"));
    let _ = std::fs::remove_dir_all(root);
}
