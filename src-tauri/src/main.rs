// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if std::env::args().any(|argument| argument == "--self-test-local-inference") {
        let executable = std::env::current_exe()?;
        let contents = executable
            .parent()
            .and_then(std::path::Path::parent)
            .ok_or("packaged executable is not inside Contents/MacOS")?;
        let resource_root = contents.join("Resources");
        let result = riimail_lib::self_test_bundled_inference(&resource_root)
            .await
            .map_err(|error| format!("bundled inference self-test failed: {error}"))?;
        println!("bundled inference self-test passed: {result}");
        return Ok(());
    }

    dotenv::dotenv().ok();

    let otlp_collector_endpoint =
        std::env::var("OTLP_COLLECTOR_ENDPOINT").unwrap_or("http://0.0.0.0:4317".to_string());

    let rust_log = std::env::var("RUST_LOG").unwrap_or("info".to_string());

    riimail_lib::tracing::init_subscriber(&rust_log, &otlp_collector_endpoint)?;

    riimail_lib::run().await;

    Ok(())
}
