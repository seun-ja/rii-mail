use crate::error::{inference_crash_handler, is_connection_error, CrashAction};
use std::io::ErrorKind;

#[test]
fn inference_crash_handler_retries_oom_with_backoff() {
    let action = inference_crash_handler(
        r#"{"message":"MODEL_OOM","reason":"gpu memory exhausted"}"#,
        2,
    );

    match action {
        CrashAction::Retry { backoff_ms, reason } => {
            assert_eq!(backoff_ms, 300);
            assert_eq!(reason, "gpu memory exhausted");
        }
        _ => panic!("expected retry action for MODEL_OOM"),
    }
}

#[test]
fn inference_crash_handler_marks_fatal() {
    let action = inference_crash_handler(r#"{"message":"MODEL_FATAL","reason":"panic"}"#, 0);
    assert!(matches!(action, CrashAction::Fatal));
}

#[test]
fn inference_crash_handler_ignores_unknown_payload() {
    let action = inference_crash_handler(r#"{"message":"UNKNOWN","reason":"na"}"#, 0);
    assert!(matches!(action, CrashAction::Ignore));
}

#[derive(Debug)]
struct WrapperError {
    source: std::io::Error,
}

impl std::fmt::Display for WrapperError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "wrapper")
    }
}

impl std::error::Error for WrapperError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

#[test]
fn is_connection_error_detects_direct_io_error() {
    let err = std::io::Error::new(ErrorKind::ConnectionReset, "reset");
    assert!(is_connection_error(&err));
}

#[test]
fn is_connection_error_walks_error_chain() {
    let err = WrapperError {
        source: std::io::Error::new(ErrorKind::BrokenPipe, "broken"),
    };

    assert!(is_connection_error(&err));
}

#[test]
fn is_connection_error_false_for_non_connection_errors() {
    let err = std::io::Error::new(ErrorKind::InvalidInput, "invalid");
    assert!(!is_connection_error(&err));
}
