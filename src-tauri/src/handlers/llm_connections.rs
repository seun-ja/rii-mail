use std::{
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use arc_swap::ArcSwap;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, Validation};
use llm::{
    providers::Providers,
    tools::{SearchEmailsTool, ToolWrapper},
};
use rand::{rngs::OsRng, RngCore as _};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest as _, Sha256};
use tauri::{Manager as _, Url};
use tauri_plugin_opener::OpenerExt as _;
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::TcpListener,
    time::{timeout, Duration},
};

use crate::{
    config::{AppState, Config},
    error::Error,
    llm::init_llm_agent,
};

const AUTHORIZE_URL: &str = "https://auth.openai.com/api/accounts/authorize";
const TOKEN_URL: &str = "https://auth.openai.com/api/accounts/oauth/token";
const JWKS_URL: &str = "https://auth.openai.com/.well-known/jwks.json";
const API_RESOURCE: &str = "https://api.openai.com/v1";
const CHATGPT_SCOPES: &str =
    "openid profile email offline_access resource.invoke chatgpt.tokens.use.direct";
const OAUTH_TIMEOUT: Duration = Duration::from_secs(300);

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmConnection {
    provider: String,
    model: String,
    account_email: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OAuthTokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    id_token: Option<String>,
    #[serde(default = "default_token_type")]
    token_type: String,
    expires_in: u64,
    #[serde(default)]
    scope: String,
}

fn default_token_type() -> String {
    "Bearer".to_string()
}

#[derive(Debug, Deserialize)]
struct IdTokenClaims {
    iss: String,
    sub: String,
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    nonce: Option<String>,
}

#[derive(Debug, Deserialize)]
struct JwkSet {
    keys: Vec<Jwk>,
}

#[derive(Debug, Deserialize)]
struct Jwk {
    kid: String,
    n: String,
    e: String,
}

#[derive(Debug)]
struct OAuthCallback {
    code: String,
    client_id: Option<String>,
}

#[derive(Debug)]
struct ChatGptCredentials {
    access_token: String,
    refresh_token: String,
    id_token: String,
    client_id: String,
    email: Option<String>,
    subject: String,
    scopes: Vec<String>,
    expires_at: u64,
    host_id: String,
    model: String,
}

#[tauri::command]
#[tracing::instrument(name = "command.llm.connect_ollama", skip_all, err)]
pub async fn connect_ollama(app: tauri::AppHandle) -> Result<LlmConnection, Error> {
    let config_path = config_path(&app)?;
    let mut config = load_config(&config_path).await?;
    let model = config.default_model.clone();
    let agent = init_llm_agent(
        Some(Providers::Ollama),
        &model,
        "ollama",
        None,
        None,
        vec![ToolWrapper::new(SearchEmailsTool)],
    )?;

    config.llm_provider = Some("ollama".to_string());
    config.save(&config_path).await.map_err(config_error)?;
    replace_llm_agent(&app, agent).await;

    Ok(LlmConnection {
        provider: "ollama".to_string(),
        model,
        account_email: None,
    })
}

#[tauri::command]
#[tracing::instrument(name = "command.llm.connect_chatgpt", skip_all, err)]
pub async fn connect_chatgpt(app: tauri::AppHandle) -> Result<LlmConnection, Error> {
    let config_path = config_path(&app)?;
    let mut config = load_config(&config_path).await?;
    let credentials = authorize_chatgpt(&app, &config).await?;

    let agent = init_llm_agent(
        Some(Providers::ChatGPT),
        &credentials.model,
        &credentials.access_token,
        None,
        None,
        Vec::<ToolWrapper<SearchEmailsTool>>::new(),
    )?;

    config.llm_provider = Some("chatgpt".to_string());
    config.default_model = credentials.model.clone();
    config.access_token = Some(credentials.access_token);
    config.refresh_token = Some(credentials.refresh_token);
    config.chatgpt.client_id = Some(credentials.client_id);
    config.chatgpt.id_token = Some(credentials.id_token);
    config.chatgpt.email = credentials.email.clone();
    config.chatgpt.subject = Some(credentials.subject);
    config.chatgpt.scopes = credentials.scopes;
    config.chatgpt.token_expires_at = Some(credentials.expires_at);
    config.chatgpt.host_id = Some(credentials.host_id);
    config.save(&config_path).await.map_err(config_error)?;
    replace_llm_agent(&app, agent).await;

    Ok(LlmConnection {
        provider: "chatgpt".to_string(),
        model: credentials.model,
        account_email: credentials.email,
    })
}

pub async fn ensure_chatgpt_agent_fresh(app: &tauri::AppHandle) -> Result<(), Error> {
    let path = config_path(app)?;
    let mut config = load_config(&path).await?;
    if config.llm_provider.as_deref() != Some("chatgpt") {
        return Ok(());
    }

    let previous_access_token = config.access_token.clone();
    refresh_chatgpt_token_if_needed(&mut config, &path).await?;
    if config.access_token == previous_access_token {
        return Ok(());
    }

    let access_token = config.access_token.as_deref().ok_or_else(|| {
        Error::Authentication("ChatGPT refresh did not return an access token".to_string())
    })?;
    let agent = init_llm_agent(
        Some(Providers::ChatGPT),
        &config.default_model,
        access_token,
        None,
        None,
        Vec::<ToolWrapper<SearchEmailsTool>>::new(),
    )?;
    replace_llm_agent(app, agent).await;
    Ok(())
}

pub async fn refresh_chatgpt_token_if_needed(
    config: &mut Config,
    path: &PathBuf,
) -> Result<(), Error> {
    if config.llm_provider.as_deref() != Some("chatgpt") {
        return Ok(());
    }

    let expires_at = config.chatgpt.token_expires_at.unwrap_or_default();
    if expires_at > unix_time().saturating_add(60) && config.access_token.is_some() {
        return Ok(());
    }

    let client_id = config.chatgpt.client_id.as_deref().ok_or_else(|| {
        Error::Authentication("ChatGPT connection is missing its client ID".to_string())
    })?;
    let refresh_token = config.refresh_token.as_deref().ok_or_else(|| {
        Error::Authentication("ChatGPT session has expired; connect it again".to_string())
    })?;

    let response = reqwest::Client::new()
        .post(TOKEN_URL)
        .form(&[
            ("grant_type", "refresh_token"),
            ("client_id", client_id),
            ("refresh_token", refresh_token),
            ("resource", API_RESOURCE),
        ])
        .send()
        .await
        .map_err(|error| Error::Authentication(format!("Could not refresh ChatGPT: {error}")))?;

    let status = response.status();
    let body = response.text().await.map_err(|error| {
        Error::Authentication(format!(
            "Could not read the ChatGPT refresh response: {error}"
        ))
    })?;
    if !status.is_success() {
        return Err(Error::Authentication(format!(
            "ChatGPT token refresh returned HTTP {status}: {body}"
        )));
    }

    let tokens: OAuthTokenResponse = serde_json::from_str(&body)?;
    let replacement_refresh_token = tokens.refresh_token.ok_or_else(|| {
        Error::Authentication("ChatGPT did not return a replacement refresh token".to_string())
    })?;
    config.access_token = Some(tokens.access_token);
    config.refresh_token = Some(replacement_refresh_token);
    if let Some(id_token) = tokens.id_token {
        config.chatgpt.id_token = Some(id_token);
    }
    if !tokens.scope.is_empty() {
        config.chatgpt.scopes = split_scopes(&tokens.scope);
    }
    config.chatgpt.token_expires_at = Some(unix_time().saturating_add(tokens.expires_in));
    config.save(path).await.map_err(config_error)
}

fn config_path(app: &tauri::AppHandle) -> Result<PathBuf, Error> {
    Ok(app.path().app_config_dir()?.join("config.json"))
}

async fn load_config(path: &PathBuf) -> Result<Config, Error> {
    Config::init(path)
        .await
        .map_err(|_| Error::Other("Configuration not found. Please run setup first.".to_string()))
}

async fn replace_llm_agent(app: &tauri::AppHandle, agent: Option<llm::Agent<crate::llm::Email>>) {
    let state = app.state::<ArcSwap<AppState>>();
    let current = state.load();
    let initialized = current.state();
    *initialized.llm_agent.write().await = agent;
}

async fn authorize_chatgpt(
    app: &tauri::AppHandle,
    config: &Config,
) -> Result<ChatGptCredentials, Error> {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
    let port = listener.local_addr()?.port();
    let redirect_uri = format!("http://127.0.0.1:{port}/auth/callback");
    let state = random_urlsafe(32);
    let nonce = random_urlsafe(32);
    let code_verifier = random_urlsafe(64);
    let code_challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(code_verifier.as_bytes()));
    let host_id = config
        .chatgpt
        .host_id
        .clone()
        .unwrap_or_else(|| format!("urn:uuid:{}", uuid::Uuid::new_v4()));
    let existing_client_id = config.chatgpt.client_id.as_deref();
    let requested_client_id = existing_client_id.unwrap_or("dynamic_agent_client");

    let mut authorize_url = Url::parse(AUTHORIZE_URL)
        .map_err(|error| Error::Other(format!("Invalid ChatGPT authorization URL: {error}")))?;
    {
        let mut query = authorize_url.query_pairs_mut();
        query
            .append_pair("client_id", requested_client_id)
            .append_pair("ext_agent_host_id", &host_id)
            .append_pair("response_type", "code")
            .append_pair("redirect_uri", &redirect_uri)
            .append_pair("scope", CHATGPT_SCOPES)
            .append_pair("resource", API_RESOURCE)
            .append_pair("state", &state)
            .append_pair("nonce", &nonce)
            .append_pair("code_challenge_method", "S256")
            .append_pair("code_challenge", &code_challenge);

        if existing_client_id.is_none() {
            query.append_pair("agent_name_hint", "RiiMail");
        } else {
            if let Some(id_token) = config.chatgpt.id_token.as_deref() {
                query.append_pair("id_token_hint", id_token);
            }
            if let Some(email) = config.chatgpt.email.as_deref() {
                query.append_pair("login_hint", email);
            }
        }
    }

    app.opener()
        .open_url(authorize_url.as_str(), None::<&str>)
        .map_err(|error| {
            Error::Other(format!("Could not open the ChatGPT sign-in page: {error}"))
        })?;

    let callback = receive_callback(listener, &state).await?;
    let client_id = match (existing_client_id, callback.client_id.as_deref()) {
        (Some(expected), Some(returned)) if expected != returned => {
            return Err(Error::Authentication(
                "ChatGPT returned a different client ID than the selected account".to_string(),
            ));
        }
        (Some(expected), _) => expected.to_string(),
        (None, Some(issued)) if issued != "dynamic_agent_client" => issued.to_string(),
        (None, _) => {
            return Err(Error::Authentication(
                "ChatGPT registration did not return an issued client ID".to_string(),
            ));
        }
    };

    let tokens = exchange_code(&callback.code, &client_id, &code_verifier, &redirect_uri).await?;
    if !tokens.token_type.eq_ignore_ascii_case("bearer") {
        return Err(Error::Authentication(format!(
            "Unsupported ChatGPT token type: {}",
            tokens.token_type
        )));
    }
    let scopes = split_scopes(&tokens.scope);
    if !scopes
        .iter()
        .any(|scope| scope == "chatgpt.tokens.use.direct")
    {
        return Err(Error::Authentication(
            "ChatGPT plan usage was not authorized for RiiMail".to_string(),
        ));
    }

    let id_token = tokens.id_token.ok_or_else(|| {
        Error::Authentication("ChatGPT did not return an identity token".to_string())
    })?;
    let claims = validate_id_token(&id_token, &client_id, &nonce).await?;
    if let Some(expected_subject) = config.chatgpt.subject.as_deref() {
        if expected_subject != claims.sub {
            return Err(Error::Authentication(
                "ChatGPT returned a different account than the selected connection".to_string(),
            ));
        }
    }
    tracing::debug!(issuer = %claims.iss, "Validated ChatGPT identity");
    let refresh_token = tokens.refresh_token.ok_or_else(|| {
        Error::Authentication("ChatGPT did not return a refresh token".to_string())
    })?;
    let model = fetch_chatgpt_model(&tokens.access_token).await?;

    Ok(ChatGptCredentials {
        access_token: tokens.access_token,
        refresh_token,
        id_token,
        client_id,
        email: claims.email,
        subject: claims.sub,
        scopes,
        expires_at: unix_time().saturating_add(tokens.expires_in),
        host_id,
        model,
    })
}

async fn receive_callback(
    listener: TcpListener,
    expected_state: &str,
) -> Result<OAuthCallback, Error> {
    let (mut stream, _) = timeout(OAUTH_TIMEOUT, listener.accept())
        .await
        .map_err(|_| Error::Authentication("ChatGPT sign-in timed out".to_string()))??;
    let mut buffer = vec![0_u8; 16 * 1024];
    let bytes_read = timeout(Duration::from_secs(10), stream.read(&mut buffer))
        .await
        .map_err(|_| Error::Authentication("ChatGPT callback timed out".to_string()))??;
    let request = String::from_utf8_lossy(&buffer[..bytes_read]);
    let target = request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .ok_or_else(|| Error::Authentication("Invalid ChatGPT callback request".to_string()))?;
    let callback_url = Url::parse(&format!("http://127.0.0.1{target}"))
        .map_err(|error| Error::Authentication(format!("Invalid ChatGPT callback URL: {error}")))?;
    if callback_url.path() != "/auth/callback" {
        return Err(Error::Authentication(
            "Unexpected ChatGPT callback path".to_string(),
        ));
    }

    let params: std::collections::HashMap<_, _> = callback_url.query_pairs().into_owned().collect();
    if params.get("state").map(String::as_str) != Some(expected_state) {
        send_browser_response(&mut stream, false).await;
        return Err(Error::Authentication(
            "ChatGPT callback state did not match the sign-in request".to_string(),
        ));
    }
    if let Some(error) = params.get("error") {
        send_browser_response(&mut stream, false).await;
        let description = params
            .get("error_description")
            .map(String::as_str)
            .unwrap_or(error);
        return Err(Error::Authentication(format!(
            "ChatGPT authorization was not completed: {description}"
        )));
    }

    let code = params
        .get("code")
        .filter(|code| !code.is_empty())
        .cloned()
        .ok_or_else(|| {
            Error::Authentication("ChatGPT callback did not contain a code".to_string())
        })?;
    send_browser_response(&mut stream, true).await;

    Ok(OAuthCallback {
        code,
        client_id: params.get("client_id").cloned(),
    })
}

async fn send_browser_response(stream: &mut tokio::net::TcpStream, success: bool) {
    let (title, message) = if success {
        (
            "ChatGPT connected",
            "Authorization received. You can return to RiiMail.",
        )
    } else {
        (
            "Could not connect ChatGPT",
            "Return to RiiMail and try again.",
        )
    };
    let body = format!(
        "<!doctype html><meta charset=\"utf-8\"><title>{title}</title><body style=\"font:16px system-ui;padding:3rem;text-align:center\"><h1>{title}</h1><p>{message}</p></body>"
    );
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(), body
    );
    let _ = stream.write_all(response.as_bytes()).await;
}

async fn exchange_code(
    code: &str,
    client_id: &str,
    code_verifier: &str,
    redirect_uri: &str,
) -> Result<OAuthTokenResponse, Error> {
    let response = reqwest::Client::new()
        .post(TOKEN_URL)
        .form(&[
            ("grant_type", "authorization_code"),
            ("client_id", client_id),
            ("code", code),
            ("code_verifier", code_verifier),
            ("redirect_uri", redirect_uri),
            ("resource", API_RESOURCE),
        ])
        .send()
        .await
        .map_err(|error| {
            Error::Authentication(format!("ChatGPT token exchange failed: {error}"))
        })?;
    let status = response.status();
    let body = response.text().await.map_err(|error| {
        Error::Authentication(format!(
            "Could not read the ChatGPT token response: {error}"
        ))
    })?;
    if !status.is_success() {
        return Err(Error::Authentication(format!(
            "ChatGPT token exchange returned HTTP {status}: {body}"
        )));
    }
    Ok(serde_json::from_str(&body)?)
}

async fn validate_id_token(
    id_token: &str,
    client_id: &str,
    expected_nonce: &str,
) -> Result<IdTokenClaims, Error> {
    let header = decode_header(id_token)
        .map_err(|error| Error::Authentication(format!("Invalid ChatGPT ID token: {error}")))?;
    let kid = header
        .kid
        .ok_or_else(|| Error::Authentication("ChatGPT ID token has no key ID".to_string()))?;
    let jwks: JwkSet = reqwest::Client::new()
        .get(JWKS_URL)
        .send()
        .await
        .map_err(|error| {
            Error::Authentication(format!("Could not fetch OpenAI signing keys: {error}"))
        })?
        .error_for_status()
        .map_err(|error| {
            Error::Authentication(format!("OpenAI signing keys were unavailable: {error}"))
        })?
        .json()
        .await
        .map_err(|error| Error::Authentication(format!("Invalid OpenAI signing keys: {error}")))?;
    let key = jwks.keys.iter().find(|key| key.kid == kid).ok_or_else(|| {
        Error::Authentication("No OpenAI signing key matched the ID token".to_string())
    })?;
    let decoding_key = DecodingKey::from_rsa_components(&key.n, &key.e)
        .map_err(|error| Error::Authentication(format!("Invalid OpenAI signing key: {error}")))?;
    let mut validation = Validation::new(Algorithm::RS256);
    validation.set_issuer(&["https://auth.openai.com"]);
    validation.set_audience(&[client_id]);
    let claims = decode::<IdTokenClaims>(id_token, &decoding_key, &validation)
        .map_err(|error| {
            Error::Authentication(format!("ChatGPT ID token validation failed: {error}"))
        })?
        .claims;
    if claims.nonce.as_deref() != Some(expected_nonce) {
        return Err(Error::Authentication(
            "ChatGPT ID token nonce did not match the sign-in request".to_string(),
        ));
    }
    Ok(claims)
}

async fn fetch_chatgpt_model(access_token: &str) -> Result<String, Error> {
    let response = reqwest::Client::new()
        .get("https://api.openai.com/v1/models")
        .bearer_auth(access_token)
        .send()
        .await
        .map_err(|error| Error::Other(format!("Could not load ChatGPT models: {error}")))?;
    let status = response.status();
    let body: Value = response
        .json()
        .await
        .map_err(|error| Error::Other(format!("Invalid ChatGPT model response: {error}")))?;
    if !status.is_success() {
        return Err(Error::Authentication(format!(
            "ChatGPT model access returned HTTP {status}: {body}"
        )));
    }

    body.get("models")
        .and_then(Value::as_array)
        .and_then(|models| {
            models.iter().find_map(|model| {
                let visible = model.get("visibility").and_then(Value::as_str) == Some("list");
                visible
                    .then(|| model.get("slug").and_then(Value::as_str))
                    .flatten()
            })
        })
        .map(str::to_string)
        .ok_or_else(|| Error::Other("No ChatGPT model is available for this account".to_string()))
}

fn random_urlsafe(length: usize) -> String {
    let mut bytes = vec![0_u8; length];
    OsRng.fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

fn split_scopes(scopes: &str) -> Vec<String> {
    scopes.split_whitespace().map(str::to_string).collect()
}

fn unix_time() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn config_error(error: Box<dyn std::error::Error + Send + Sync>) -> Error {
    Error::Other(format!("Could not save configuration: {error}"))
}

#[cfg(test)]
mod tests {
    use super::{split_scopes, CHATGPT_SCOPES};

    #[test]
    fn requested_scopes_include_direct_plan_usage_and_refresh() {
        let scopes = split_scopes(CHATGPT_SCOPES);
        assert!(scopes
            .iter()
            .any(|scope| scope == "chatgpt.tokens.use.direct"));
        assert!(scopes.iter().any(|scope| scope == "offline_access"));
    }
}
