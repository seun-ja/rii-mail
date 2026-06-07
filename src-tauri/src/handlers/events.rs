use std::sync::atomic::AtomicU8;
use tauri::{AppHandle, Emitter as _, Manager};
use tokio::sync::Mutex;
use tokio::time::{self, Duration};

use crate::error::Error;

#[tauri::command]
pub async fn email_populated(app: AppHandle) -> Result<(), Error> {
    let progress_state = app.state::<Mutex<AtomicU8>>();

    let mut ticker = time::interval(Duration::from_millis(100));

    loop {
        let progress = progress_state
            .lock()
            .await
            .load(std::sync::atomic::Ordering::Relaxed);

        if progress >= 100 {
            break;
        }

        app.emit("population-progress", progress)?;

        ticker.tick().await;
    }

    Ok(())
}
