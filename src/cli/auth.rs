use std::io::ErrorKind;
use std::time::Duration;

use chrono::{DateTime, SecondsFormat, Utc};
use clap::{Args, Subcommand};
use serde::Serialize;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use url::Url;

use crate::client::{AccountIdentity, OAuthTokenResponse, ThreadsClient};
use crate::config::AppConfig;
use crate::error::{CliError, ErrorCategory};
use crate::output::{print_success, OutputMode};
use crate::store::{retry_with_backoff, RetryOperation, RetryPolicy, Store};

#[derive(Debug, Subcommand)]
pub enum AuthSubcommand {
    Login(LoginArgs),
    Refresh,
    Status,
    Logout(LogoutArgs),
}

#[derive(Debug, Args)]
pub struct LoginArgs {
    #[arg(long, default_value_t = false)]
    pub no_browser: bool,
}

#[derive(Debug, Args)]
pub struct LogoutArgs {
    #[arg(long, default_value_t = false)]
    pub yes: bool,
}

#[derive(Debug, Serialize)]
struct AuthStatusData {
    authenticated: bool,
    token_id: Option<i64>,
    account_id: Option<i64>,
    account_username: Option<String>,
    account_name: Option<String>,
    expires_at: Option<String>,
    token_expired: Option<bool>,
    usable: bool,
    token_created_at: Option<String>,
}

#[derive(Debug, Serialize)]
struct LogoutData {
    removed_token_rows: usize,
}

#[derive(Debug, Serialize)]
struct AuthLifecycleData {
    account_id: i64,
    token_id: i64,
    expires_at: Option<String>,
}

pub async fn run(
    command: super::AuthCommand,
    app: &AppConfig,
    store: &Store,
    output_mode: OutputMode,
) -> Result<(), CliError> {
    match command.command {
        AuthSubcommand::Login(args) => run_login(args, app, store, output_mode).await,
        AuthSubcommand::Refresh => run_refresh(app, store, output_mode).await,
        AuthSubcommand::Status => run_status(store, output_mode),
        AuthSubcommand::Logout(args) => run_logout(args, store, output_mode),
    }
}

async fn run_login(
    args: LoginArgs,
    app: &AppConfig,
    store: &Store,
    output_mode: OutputMode,
) -> Result<(), CliError> {
    ensure_localhost_callback_host(&app.oauth.listen_host)?;
    let client = ThreadsClient::from_config(app)?;
    let state = uuid::Uuid::new_v4().to_string();
    let auth_url = build_authorize_url(app, &state)?;

    let listener_addr = format!("{}:{}", app.oauth.listen_host, app.oauth.listen_port);
    let listener = TcpListener::bind(&listener_addr).await.map_err(|e| {
        CliError::new(
            ErrorCategory::Network,
            format!("failed binding OAuth callback listener on {}: {}", listener_addr, e),
        )
    })?;

    if app.defaults.open_browser && !args.no_browser {
        webbrowser::open(auth_url.as_str()).map_err(|e| {
            CliError::new(
                ErrorCategory::Config,
                format!("failed to open browser: {}", e),
            )
        })?;
    } else {
        eprintln!("Open this URL to continue login:\n{}", auth_url);
    }

    let callback = wait_for_callback(listener, &state, app.oauth.state_ttl_seconds).await?;

    let mut token = client
        .exchange_oauth_token(&callback.code, &app.threads.redirect_uri)
        .await?;

    // Optional long-lived exchange: only attempt when refresh token is absent.
    if token.refresh_token.is_none() {
        if let Ok(long_lived) = client.exchange_long_lived_token(&token.access_token).await {
            token = long_lived;
        }
    }

    let identity = client.fetch_account_identity(&token.access_token).await?;
    let persisted = persist_identity_and_token(store, &identity, &token, None)?;

    print_success(
        output_mode,
        "auth login",
        "OAuth login completed and token saved.",
        AuthLifecycleData {
            account_id: persisted.account_id,
            token_id: persisted.token_id,
            expires_at: persisted.expires_at,
        },
    );
    Ok(())
}

fn ensure_localhost_callback_host(host: &str) -> Result<(), CliError> {
    if host == "127.0.0.1" || host == "localhost" {
        Ok(())
    } else {
        Err(CliError::new(
            ErrorCategory::Config,
            "oauth.listen_host must be localhost-only (127.0.0.1 or localhost)",
        ))
    }
}

async fn run_refresh(app: &AppConfig, store: &Store, output_mode: OutputMode) -> Result<(), CliError> {
    let existing = store.latest_token()?.ok_or_else(|| {
        CliError::new(
            ErrorCategory::Auth,
            "no stored token found; run auth login first",
        )
    })?;

    let refresh_token = existing.refresh_token.as_deref().ok_or_else(|| {
        CliError::new(
            ErrorCategory::Auth,
            "stored token has no refresh_token; run auth login again",
        )
    })?;

    let client = ThreadsClient::from_config(app)?;
    let token = retry_with_backoff(
        RetryPolicy {
            max_retries: 3,
            base_delay_ms: 100,
        },
        RetryOperation::TokenRefresh,
        || async { client.refresh_oauth_token(refresh_token).await },
    )
    .await?;
    let identity = client.fetch_account_identity(&token.access_token).await?;
    let persisted = persist_identity_and_token(store, &identity, &token, Some(refresh_token))?;

    print_success(
        output_mode,
        "auth refresh",
        "Token refresh completed and token saved.",
        AuthLifecycleData {
            account_id: persisted.account_id,
            token_id: persisted.token_id,
            expires_at: persisted.expires_at,
        },
    );
    Ok(())
}

fn run_status(store: &Store, output_mode: OutputMode) -> Result<(), CliError> {
    let token = store.latest_token()?;
    let data = if let Some(t) = token {
        let account = store.get_account_by_id(t.account_id)?;
        let expired = token_expired(t.expires_at.as_deref());
        let account_present = account.is_some();
        AuthStatusData {
            authenticated: true,
            token_id: Some(t.id),
            account_id: Some(t.account_id),
            account_username: account.as_ref().and_then(|a| a.username.clone()),
            account_name: account.and_then(|a| a.name),
            expires_at: t.expires_at,
            token_expired: Some(expired),
            usable: account_present && !expired,
            token_created_at: Some(t.created_at),
        }
    } else {
        AuthStatusData {
            authenticated: false,
            token_id: None,
            account_id: None,
            account_username: None,
            account_name: None,
            expires_at: None,
            token_expired: None,
            usable: false,
            token_created_at: None,
        }
    };

    let human = if data.authenticated {
        if data.usable {
            "Auth status: authenticated and token is usable."
        } else {
            "Auth status: token is present but expired; run auth refresh or auth login."
        }
    } else {
        "Auth status: no token found."
    };
    print_success(output_mode, "auth status", human, data);
    Ok(())
}

fn run_logout(args: LogoutArgs, store: &Store, output_mode: OutputMode) -> Result<(), CliError> {
    if !args.yes {
        return Err(CliError::new(
            ErrorCategory::Validation,
            "auth logout requires --yes",
        ));
    }
    let removed = store.delete_all_tokens()?;
    print_success(
        output_mode,
        "auth logout",
        format!("Removed {} token row(s).", removed),
        LogoutData {
            removed_token_rows: removed,
        },
    );
    Ok(())
}

#[derive(Debug)]
struct CallbackPayload {
    code: String,
}

async fn wait_for_callback(
    listener: TcpListener,
    expected_state: &str,
    ttl_seconds: u64,
) -> Result<CallbackPayload, CliError> {
    let timeout = Duration::from_secs(ttl_seconds.max(1));
    let (mut socket, _) = tokio::time::timeout(timeout, listener.accept())
        .await
        .map_err(|_| CliError::new(ErrorCategory::Auth, "timed out waiting for OAuth callback"))?
        .map_err(|e| CliError::new(ErrorCategory::Network, format!("OAuth callback accept failed: {}", e)))?;

    let mut buffer = vec![0_u8; 4096];
    let size = socket.read(&mut buffer).await.map_err(|e| {
        CliError::new(
            ErrorCategory::Network,
            format!("failed reading OAuth callback request: {}", e),
        )
    })?;
    if size == 0 {
        return Err(CliError::new(
            ErrorCategory::Auth,
            "empty OAuth callback request",
        ));
    }

    let request = String::from_utf8_lossy(&buffer[..size]);
    let parsed = parse_callback_request(&request, expected_state)?;

    write_callback_response(
        &mut socket,
        "Authentication complete. You can close this tab.",
    )
    .await?;

    Ok(parsed)
}

fn parse_callback_request(request: &str, expected_state: &str) -> Result<CallbackPayload, CliError> {
    let first_line = request
        .lines()
        .next()
        .ok_or_else(|| CliError::new(ErrorCategory::Auth, "invalid OAuth callback request"))?;

    let mut parts = first_line.split_whitespace();
    let method = parts.next().unwrap_or_default();
    if method != "GET" {
        return Err(CliError::new(
            ErrorCategory::Auth,
            "OAuth callback must be a GET request",
        ));
    }

    let target = parts.next().ok_or_else(|| {
        CliError::new(
            ErrorCategory::Auth,
            "OAuth callback request is missing target path",
        )
    })?;

    let callback_url = Url::parse(&format!("http://localhost{}", target)).map_err(|e| {
        CliError::new(
            ErrorCategory::Auth,
            format!("invalid OAuth callback target: {}", e),
        )
    })?;

    if let Some(error) = callback_url
        .query_pairs()
        .find_map(|(k, v)| (k == "error").then_some(v.into_owned()))
    {
        return Err(CliError::new(
            ErrorCategory::Auth,
            format!("OAuth authorization failed: {}", error),
        ));
    }

    let code = callback_url
        .query_pairs()
        .find_map(|(k, v)| (k == "code").then_some(v.into_owned()))
        .ok_or_else(|| CliError::new(ErrorCategory::Auth, "OAuth callback missing code"))?;

    let state = callback_url
        .query_pairs()
        .find_map(|(k, v)| (k == "state").then_some(v.into_owned()))
        .ok_or_else(|| CliError::new(ErrorCategory::Auth, "OAuth callback missing state"))?;

    if state != expected_state {
        return Err(CliError::new(
            ErrorCategory::Auth,
            "OAuth callback state mismatch",
        ));
    }

    Ok(CallbackPayload { code })
}

async fn write_callback_response(
    socket: &mut tokio::net::TcpStream,
    message: &str,
) -> Result<(), CliError> {
    let body = format!("<html><body><p>{}</p></body></html>", message);
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    match socket.write_all(response.as_bytes()).await {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == ErrorKind::BrokenPipe => Ok(()),
        Err(err) => Err(CliError::new(
            ErrorCategory::Network,
            format!("failed writing OAuth callback response: {}", err),
        )),
    }
}

fn build_authorize_url(app: &AppConfig, state: &str) -> Result<Url, CliError> {
    let mut url = Url::parse(&format!(
        "{}/{}/oauth/authorize",
        app.threads.base_url.trim_end_matches('/'),
        app.threads.version.trim_start_matches('/'),
    ))
    .map_err(|e| {
        CliError::new(
            ErrorCategory::Config,
            format!("invalid authorize URL base: {}", e),
        )
    })?;

    url.query_pairs_mut()
        .append_pair("client_id", &app.threads.app_id)
        .append_pair("redirect_uri", &app.threads.redirect_uri)
        .append_pair("response_type", "code")
        .append_pair("scope", "threads_basic,threads_content_publish,threads_manage_replies,threads_manage_insights,threads_keyword_search")
        .append_pair("state", state);
    Ok(url)
}

#[derive(Debug)]
struct PersistedToken {
    account_id: i64,
    token_id: i64,
    expires_at: Option<String>,
}

fn persist_identity_and_token(
    store: &Store,
    identity: &AccountIdentity,
    token: &OAuthTokenResponse,
    fallback_refresh_token: Option<&str>,
) -> Result<PersistedToken, CliError> {
    let account_id = store.upsert_account(
        &identity.id,
        identity.username.as_deref(),
        identity.name.as_deref(),
    )?;

    let now = Utc::now();
    let issued_at = now.to_rfc3339_opts(SecondsFormat::Secs, true);
    let expires_at = compute_expires_at(token.expires_in, now);
    let refresh_token = token
        .refresh_token
        .as_deref()
        .or(fallback_refresh_token);
    let raw_json = serde_json::to_string(token).map_err(|e| {
        CliError::new(
            ErrorCategory::Internal,
            format!("failed serializing token payload for storage: {}", e),
        )
    })?;

    let token_id = store.insert_token(
        account_id,
        &token.access_token,
        refresh_token,
        Some(&issued_at),
        expires_at.as_deref(),
        &raw_json,
    )?;

    Ok(PersistedToken {
        account_id,
        token_id,
        expires_at,
    })
}

fn compute_expires_at(expires_in: Option<i64>, issued_at: DateTime<Utc>) -> Option<String> {
    expires_in
        .and_then(chrono::Duration::try_seconds)
        .and_then(|delta| issued_at.checked_add_signed(delta))
        .map(|dt| dt.to_rfc3339_opts(SecondsFormat::Secs, true))
}

fn token_expired(expires_at: Option<&str>) -> bool {
    match expires_at {
        Some(value) => DateTime::parse_from_rfc3339(value)
            .map(|dt| dt.with_timezone(&Utc) <= Utc::now())
            .unwrap_or(false),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    use crate::config::{DefaultsConfig, OAuthConfig, StorageConfig, ThreadsConfig};

    fn test_config(db_path: &str, base_url: &str) -> AppConfig {
        AppConfig {
            threads: ThreadsConfig {
                app_id: "app-id".to_string(),
                app_secret_file: db_path.replace("threads.db", "secret.txt"),
                redirect_uri: "http://127.0.0.1:8788/callback".to_string(),
                user_id: "user-id".to_string(),
                base_url: base_url.to_string(),
                version: "v1.0".to_string(),
            },
            storage: StorageConfig {
                database_path: db_path.to_string(),
            },
            defaults: DefaultsConfig {
                link_mode: "reply".to_string(),
                output: "human".to_string(),
                open_browser: false,
            },
            oauth: OAuthConfig {
                listen_host: "127.0.0.1".to_string(),
                listen_port: 8788,
                state_ttl_seconds: 60,
            },
        }
    }

    fn write_secret_file(app: &AppConfig) {
        std::fs::write(&app.threads.app_secret_file, "test-secret").expect("write secret file");
    }

    fn spawn_mock_server(token_body: serde_json::Value, me_body: serde_json::Value) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind server");
        let addr = listener.local_addr().expect("local addr");
        thread::spawn(move || {
            for idx in 0..2 {
                let (mut stream, _) = listener.accept().expect("accept");
                let mut request = Vec::new();
                let mut temp = [0u8; 1024];
                loop {
                    let n = stream.read(&mut temp).expect("read");
                    if n == 0 {
                        break;
                    }
                    request.extend_from_slice(&temp[..n]);
                    if request.windows(4).any(|w| w == b"\r\n\r\n") {
                        break;
                    }
                }
                let req_text = String::from_utf8_lossy(&request);
                let first_line = req_text.lines().next().unwrap_or_default();
                let path = first_line.split_whitespace().nth(1).unwrap_or("/");
                let body = if idx == 0 && path.contains("/oauth/access_token") {
                    token_body.to_string()
                } else if idx == 1 && path.contains("/me?") {
                    me_body.to_string()
                } else {
                    serde_json::json!({"error":{"message":"not found"}}).to_string()
                };
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                stream.write_all(response.as_bytes()).expect("write response");
                stream.flush().expect("flush");
            }
        });
        format!("http://{}", addr)
    }

    #[test]
    fn parse_callback_validates_state() {
        let req = "GET /callback?code=abc&state=expected HTTP/1.1\r\nHost: localhost\r\n\r\n";
        let parsed = parse_callback_request(req, "expected").expect("callback parsed");
        assert_eq!(parsed.code, "abc");

        let err = parse_callback_request(req, "wrong").expect_err("must reject wrong state");
        assert_eq!(err.category.as_code(), "AUTH_ERROR");
        assert_eq!(err.message, "OAuth callback state mismatch");
    }

    #[test]
    fn parse_callback_surfaces_oauth_error() {
        let req = "GET /callback?error=access_denied&state=expected HTTP/1.1\r\nHost: localhost\r\n\r\n";
        let err = parse_callback_request(req, "expected").expect_err("callback should fail");
        assert_eq!(err.category.as_code(), "AUTH_ERROR");
        assert_eq!(err.message, "OAuth authorization failed: access_denied");
    }

    #[test]
    fn parse_callback_rejects_non_get_and_missing_params() {
        let post_req = "POST /callback?code=abc&state=expected HTTP/1.1\r\nHost: localhost\r\n\r\n";
        let err = parse_callback_request(post_req, "expected").expect_err("must reject non-get method");
        assert_eq!(err.category.as_code(), "AUTH_ERROR");
        assert_eq!(err.message, "OAuth callback must be a GET request");

        let missing_code = "GET /callback?state=expected HTTP/1.1\r\nHost: localhost\r\n\r\n";
        let err = parse_callback_request(missing_code, "expected").expect_err("must require code");
        assert_eq!(err.category.as_code(), "AUTH_ERROR");
        assert_eq!(err.message, "OAuth callback missing code");

        let missing_state = "GET /callback?code=abc HTTP/1.1\r\nHost: localhost\r\n\r\n";
        let err = parse_callback_request(missing_state, "expected").expect_err("must require state");
        assert_eq!(err.category.as_code(), "AUTH_ERROR");
        assert_eq!(err.message, "OAuth callback missing state");
    }

    #[test]
    fn persists_account_and_token_payload() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("threads.db");
        let store = Store::open(db_path.to_str().expect("db path")).expect("store open");
        store.run_migrations().expect("migrations");

        let identity = AccountIdentity {
            id: "acct-1".to_string(),
            username: Some("alice".to_string()),
            name: Some("Alice".to_string()),
        };
        let token = OAuthTokenResponse {
            access_token: "secret-access".to_string(),
            token_type: Some("bearer".to_string()),
            expires_in: Some(3600),
            refresh_token: Some("secret-refresh".to_string()),
        };

        let persisted =
            persist_identity_and_token(&store, &identity, &token, None).expect("persist token");

        assert!(persisted.account_id > 0);
        assert!(persisted.token_id > 0);

        let latest = store
            .latest_token()
            .expect("latest token")
            .expect("token row");
        assert_eq!(latest.account_id, persisted.account_id);
        assert_eq!(latest.refresh_token.as_deref(), Some("secret-refresh"));
        assert!(latest.raw_json.contains("secret-access"));
    }

    #[test]
    fn persist_uses_fallback_refresh_token() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("threads.db");
        let store = Store::open(db_path.to_str().expect("db path")).expect("store open");
        store.run_migrations().expect("migrations");

        let identity = AccountIdentity {
            id: "acct-1".to_string(),
            username: Some("alice".to_string()),
            name: Some("Alice".to_string()),
        };
        let token = OAuthTokenResponse {
            access_token: "new-access".to_string(),
            token_type: Some("bearer".to_string()),
            expires_in: Some(3600),
            refresh_token: None,
        };

        persist_identity_and_token(&store, &identity, &token, Some("existing-refresh"))
            .expect("persist token");

        let latest = store
            .latest_token()
            .expect("latest token")
            .expect("token row");
        assert_eq!(latest.refresh_token.as_deref(), Some("existing-refresh"));
    }

    #[test]
    fn logout_requires_yes_flag() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("threads.db");
        let store = Store::open(db_path.to_str().expect("db path")).expect("store open");
        store.run_migrations().expect("migrations");

        let err = run_logout(LogoutArgs { yes: false }, &store, OutputMode::Json)
            .expect_err("logout must require --yes");
        assert_eq!(err.category.as_code(), "VALIDATION_ERROR");
        assert_eq!(err.message, "auth logout requires --yes");
    }

    #[test]
    fn callback_listener_host_must_be_localhost_only() {
        ensure_localhost_callback_host("127.0.0.1").expect("localhost ip should pass");
        ensure_localhost_callback_host("localhost").expect("localhost name should pass");
        let err = ensure_localhost_callback_host("0.0.0.0").expect_err("must reject wildcard host");
        assert_eq!(err.category.as_code(), "CONFIG_ERROR");
        assert_eq!(
            err.message,
            "oauth.listen_host must be localhost-only (127.0.0.1 or localhost)"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn refresh_keeps_existing_refresh_token_when_api_omits_it() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("threads.db");
        let db_path_str = db_path.to_str().expect("db path");
        let base_url = spawn_mock_server(
            serde_json::json!({"access_token":"new-access","token_type":"bearer","expires_in":3600}),
            serde_json::json!({"id":"acct-1","username":"alice","name":"Alice"}),
        );
        let app = test_config(db_path_str, &base_url);
        write_secret_file(&app);
        let store = Store::open(db_path_str).expect("store open");
        store.run_migrations().expect("migrations");

        let account_id = store
            .upsert_account("acct-1", Some("alice"), Some("Alice"))
            .expect("upsert account");
        store
            .insert_token(
                account_id,
                "old-access",
                Some("old-refresh"),
                Some("2024-01-01T00:00:00Z"),
                None,
                "{}",
            )
            .expect("insert original token");

        run_refresh(&app, &store, OutputMode::Json)
            .await
            .expect("refresh should succeed");

        let latest = store
            .latest_token()
            .expect("latest token")
            .expect("token row");
        assert_eq!(latest.access_token, "new-access");
        assert_eq!(latest.refresh_token.as_deref(), Some("old-refresh"));
    }

    #[test]
    fn logout_yes_removes_all_tokens() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("threads.db");
        let store = Store::open(db_path.to_str().expect("db path")).expect("store open");
        store.run_migrations().expect("migrations");

        let account_id = store
            .upsert_account("acct-1", Some("alice"), Some("Alice"))
            .expect("upsert account");
        store
            .insert_token(account_id, "access-1", None, None, None, "{}")
            .expect("insert token 1");
        store
            .insert_token(account_id, "access-2", None, None, None, "{}")
            .expect("insert token 2");

        run_logout(LogoutArgs { yes: true }, &store, OutputMode::Json).expect("logout should succeed");
        assert!(store.latest_token().expect("latest token").is_none());
    }
}
