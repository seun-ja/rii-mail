use crate::config::{AppState, Config, InitStatus};
use std::time::{SystemTime, UNIX_EPOCH};

fn unique_config_path(prefix: &str) -> std::path::PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be monotonic for tests")
        .as_nanos();
    std::env::temp_dir().join(format!("riimail-{prefix}-{nanos}.json"))
}

#[tokio::test]
async fn config_init_reads_valid_config() {
    let path = unique_config_path("config-valid");
    let body = r#"{
        "imap_server_url":"imap.gmail.com",
        "smtp_relay_url":"smtp.gmail.com",
        "imap_port":993,
        "sqlite_db":"emails.db",
        "accounts":["alice@example.com"],
        "otlp_collector_endpoint":"http://127.0.0.1:4317",
        "provider": "ollama",
        "default_model": "model",
        "api_key": "api_key"
    }"#;

    tokio::fs::write(&path, body)
        .await
        .expect("config file should be written");

    let parsed = Config::init(&path).await.expect("config should parse");

    assert_eq!(parsed.imap_server_url, "imap.gmail.com");
    assert_eq!(parsed.imap_port, 993);
    assert_eq!(parsed.accounts.len(), 1);

    let _ = tokio::fs::remove_file(path).await;
}

#[test]
fn chatgpt_config_is_grouped_without_changing_stored_field_names() {
    let body = r#"{
        "imap_server_url":"imap.gmail.com",
        "smtp_relay_url":"smtp.gmail.com",
        "imap_port":993,
        "sqlite_db":"emails.db",
        "accounts":[],
        "default_model":"model",
        "api_key":"",
        "chatgpt_client_id":"oaiapp_test",
        "chatgpt_email":"alice@example.com",
        "chatgpt_scopes":["openid","resource.invoke"]
    }"#;

    let parsed: Config = serde_json::from_str(body).expect("flat config should deserialize");
    assert_eq!(parsed.chatgpt.client_id.as_deref(), Some("oaiapp_test"));
    assert_eq!(parsed.chatgpt.email.as_deref(), Some("alice@example.com"));

    let serialized = serde_json::to_value(parsed).expect("config should serialize");
    assert_eq!(serialized["chatgpt_client_id"], "oaiapp_test");
    assert!(serialized.get("chatgpt").is_none());
}

#[tokio::test]
async fn config_init_rejects_missing_imap_values() {
    let path = unique_config_path("config-invalid");
    let body = r#"{
        "imap_server_url":"",
        "smtp_relay_url":"smtp.gmail.com",
        "imap_port":0,
        "sqlite_db":"emails.db",
        "accounts":[],
        "otlp_collector_endpoint":null,
        "provider": "ollama",
        "default_model": "model",
        "api_key": "api_key"
    }"#;

    tokio::fs::write(&path, body)
        .await
        .expect("config file should be written");

    let err = match Config::init(&path).await {
        Ok(_) => panic!("config with empty IMAP settings should fail"),
        Err(err) => err,
    };

    assert!(err.to_string().contains("IMAP Port and Server not set"));

    let _ = tokio::fs::remove_file(path).await;
}

#[test]
fn init_status_serializes_to_snake_case() {
    let serialized =
        serde_json::to_string(&InitStatus::ReturningSigned).expect("status should serialize");
    assert_eq!(serialized, "\"returning_signed\"");
}

#[test]
fn app_state_fresh_panics_on_state_access() {
    let result = std::panic::catch_unwind(|| {
        let state = AppState::Fresh;
        let _ = state.state();
    });

    assert!(result.is_err());
}
