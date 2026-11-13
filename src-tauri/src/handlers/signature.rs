use std::path::PathBuf;

use tauri::Manager as _;

use crate::{config::Config, error::Error};

fn normalize_signature(signature: Option<String>) -> Option<String> {
    signature.filter(|value| !value.trim().is_empty())
}

fn legacy_signature_to_html(signature: &str) -> String {
    let escaped = signature
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('\n', "<br>");
    format!("<span>{escaped}</span>")
}

fn config_path(app: &tauri::AppHandle) -> Result<PathBuf, Error> {
    Ok(app.path().app_config_dir()?.join("config.json"))
}

async fn load_config(path: &PathBuf) -> Result<Config, Error> {
    Config::init(path)
        .await
        .map_err(|_| Error::Other("Configuration not found. Please run setup first.".to_string()))
}

#[tauri::command]
#[tracing::instrument(name = "command.signature.get", skip_all, err)]
pub async fn get_signature(app: tauri::AppHandle) -> Result<Option<String>, Error> {
    let path = config_path(&app)?;
    let config = load_config(&path).await?;

    Ok(normalize_signature(config.signature_html).or_else(|| {
        normalize_signature(config.signature).map(|signature| legacy_signature_to_html(&signature))
    }))
}

#[tauri::command]
#[tracing::instrument(name = "command.signature.set", skip_all, err)]
pub async fn set_signature(app: tauri::AppHandle, signature: Option<String>) -> Result<(), Error> {
    let path = config_path(&app)?;
    let signature_html = normalize_signature(signature);
    Config::update(&path, move |config| {
        config.signature_html = signature_html;
        config.signature = None;
    })
    .await
    .map(|_| ())
    .map_err(|error| Error::Other(format!("Could not save configuration: {error}")))
}

#[cfg(test)]
mod tests {
    use super::{legacy_signature_to_html, normalize_signature};

    #[test]
    fn empty_and_whitespace_only_signatures_are_disabled() {
        assert_eq!(normalize_signature(None), None);
        assert_eq!(normalize_signature(Some(String::new())), None);
        assert_eq!(normalize_signature(Some(" \n\t ".to_string())), None);
    }

    #[test]
    fn non_empty_rich_signature_is_preserved_exactly() {
        let signature =
            "<strong>Best regards,</strong><br><a href=\"https://example.com\">Aminu</a>"
                .to_string();

        assert_eq!(
            normalize_signature(Some(signature.clone())),
            Some(signature)
        );
    }

    #[test]
    fn legacy_plain_text_signatures_are_escaped_and_keep_line_breaks() {
        assert_eq!(
            legacy_signature_to_html("Thanks <team> & all\r\nAminu"),
            "<span>Thanks &lt;team&gt; &amp; all<br>Aminu</span>"
        );
    }
}
