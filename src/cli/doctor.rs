use clap::Subcommand;
use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::config::AppConfig;
use crate::client::ThreadsClient;
use crate::error::{CliError, ErrorCategory};
use crate::output::{print_success, OutputMode};
use crate::store::Store;

#[derive(Debug, Subcommand)]
pub enum DoctorSubcommand {
    Check,
}

#[derive(Debug, Serialize)]
struct DoctorData {
    config_path: String,
    checks: DoctorChecks,
    auth: AuthReadiness,
    summary: DoctorSummary,
}

#[derive(Debug, Serialize)]
struct DoctorChecks {
    config_parse_ok: bool,
    config_required_fields_ok: bool,
    database_path: String,
    sqlite_open_ok: bool,
    sqlite_migrations_ok: bool,
    app_secret_file_accessible: bool,
    oauth_localhost_only: bool,
    client_base_url: String,
    client_api_version: String,
    client_ready: bool,
}

#[derive(Debug, Serialize)]
struct AuthReadiness {
    token_present: bool,
    account_present: bool,
    token_expired: Option<bool>,
    ready: bool,
}

#[derive(Debug, Serialize)]
struct DoctorSummary {
    ok: bool,
    failed_checks: Vec<String>,
}

pub fn run(
    command: super::DoctorCommand,
    app: &AppConfig,
    store: &Store,
    output_mode: OutputMode,
) -> Result<(), CliError> {
    match command.command {
        DoctorSubcommand::Check => {
            let config_path_buf = AppConfig::effective_config_path(None)?;
            let config_path = config_path_buf.display().to_string();
            let db_path = store.db_path().display().to_string();
            let config_parse_ok = AppConfig::load(Some(&config_path)).is_ok();
            let config_required_fields_ok = app.validate().is_ok();
            let sqlite_open_ok = store.connection().is_ok();
            let sqlite_migrations_ok = store.run_migrations().is_ok();
            let app_secret_ok = app.read_app_secret().is_ok();
            let oauth_local = app.oauth.listen_host == "127.0.0.1" || app.oauth.listen_host == "localhost";
            let client = ThreadsClient::from_config(app)?;
            let health = client.health();
            let latest_token = store.latest_token()?;
            let token_present = latest_token.is_some();
            let account_present = if let Some(token) = latest_token.as_ref() {
                store.get_account_by_id(token.account_id)?.is_some()
            } else {
                false
            };
            let token_expired = latest_token
                .as_ref()
                .and_then(|t| t.expires_at.as_deref())
                .map(is_expired);
            let auth_ready = token_present && account_present && token_expired != Some(true);

            let checks = DoctorChecks {
                config_parse_ok,
                config_required_fields_ok,
                database_path: db_path,
                sqlite_open_ok,
                sqlite_migrations_ok,
                app_secret_file_accessible: app_secret_ok,
                oauth_localhost_only: oauth_local,
                client_base_url: health.base_url,
                client_api_version: health.api_version,
                client_ready: health.http_ready,
            };
            let auth = AuthReadiness {
                token_present,
                account_present,
                token_expired,
                ready: auth_ready,
            };
            let summary = DoctorSummary {
                ok: false,
                failed_checks: collect_failed_checks(&checks, &auth),
            };
            let data = DoctorData {
                config_path,
                checks,
                auth,
                summary: DoctorSummary {
                    ok: summary.failed_checks.is_empty(),
                    failed_checks: summary.failed_checks,
                },
            };
            if !data.summary.ok {
                return Err(CliError::new(
                    ErrorCategory::Config,
                    format!(
                        "doctor checks failed: {}",
                        data.summary.failed_checks.join(", ")
                    ),
                ));
            }
            let human = format!(
                "doctor check: ok (config={}, sqlite=open+migrations, auth_ready={})",
                data.config_path, data.auth.ready
            );
            print_success(output_mode, "doctor check", human, data);
            Ok(())
        }
    }
}

fn collect_failed_checks(checks: &DoctorChecks, auth: &AuthReadiness) -> Vec<String> {
    let mut failed = Vec::new();
    if !checks.config_parse_ok {
        failed.push("config_parse".to_string());
    }
    if !checks.config_required_fields_ok {
        failed.push("config_required_fields".to_string());
    }
    if !checks.sqlite_open_ok {
        failed.push("sqlite_open".to_string());
    }
    if !checks.sqlite_migrations_ok {
        failed.push("sqlite_migrations".to_string());
    }
    if !checks.app_secret_file_accessible {
        failed.push("app_secret_file".to_string());
    }
    if !checks.oauth_localhost_only {
        failed.push("oauth_localhost".to_string());
    }
    if !checks.client_ready {
        failed.push("client_ready".to_string());
    }
    if !auth.ready {
        failed.push("auth_readiness".to_string());
    }
    failed
}

fn is_expired(expires_at: &str) -> bool {
    DateTime::parse_from_rfc3339(expires_at)
        .map(|dt| dt.with_timezone(&Utc) <= Utc::now())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AppConfig, DefaultsConfig, OAuthConfig, StorageConfig, ThreadsConfig};

    fn sample_config(db: String, secret: String) -> AppConfig {
        AppConfig {
            threads: ThreadsConfig {
                app_id: "id".to_string(),
                app_secret_file: secret,
                redirect_uri: "http://127.0.0.1:8788/callback".to_string(),
                user_id: "u1".to_string(),
                base_url: "https://graph.threads.net".to_string(),
                version: "v1.0".to_string(),
            },
            storage: StorageConfig { database_path: db },
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

    #[test]
    fn collect_failed_checks_reports_expected_keys() {
        let checks = DoctorChecks {
            config_parse_ok: false,
            config_required_fields_ok: true,
            database_path: "/tmp/db.sqlite".to_string(),
            sqlite_open_ok: false,
            sqlite_migrations_ok: true,
            app_secret_file_accessible: false,
            oauth_localhost_only: true,
            client_base_url: "https://graph.threads.net".to_string(),
            client_api_version: "v1.0".to_string(),
            client_ready: true,
        };
        let auth = AuthReadiness {
            token_present: false,
            account_present: false,
            token_expired: None,
            ready: false,
        };
        let failed = collect_failed_checks(&checks, &auth);
        assert!(failed.contains(&"config_parse".to_string()));
        assert!(failed.contains(&"sqlite_open".to_string()));
        assert!(failed.contains(&"app_secret_file".to_string()));
        assert!(failed.contains(&"auth_readiness".to_string()));
    }

    #[test]
    fn doctor_fails_without_auth_token() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("threads.db");
        let secret_path = dir.path().join("secret.txt");
        std::fs::write(&secret_path, "secret").expect("write secret");
        let config = sample_config(
            db_path.to_string_lossy().to_string(),
            secret_path.to_string_lossy().to_string(),
        );
        let store = Store::open(&config.storage.database_path).expect("store open");
        store.run_migrations().expect("migrations");
        let result = run(
            super::super::DoctorCommand {
                command: DoctorSubcommand::Check,
            },
            &config,
            &store,
            OutputMode::Json,
        );
        let err = result.expect_err("doctor should fail without auth token");
        assert_eq!(err.category.as_code(), "CONFIG_ERROR");
        assert!(err.message.contains("auth_readiness"));
    }

    #[test]
    fn doctor_passes_when_auth_ready_and_localhost_valid() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("threads.db");
        let secret_path = dir.path().join("secret.txt");
        std::fs::write(&secret_path, "secret").expect("write secret");
        let config = sample_config(
            db_path.to_string_lossy().to_string(),
            secret_path.to_string_lossy().to_string(),
        );
        let store = Store::open(&config.storage.database_path).expect("store open");
        store.run_migrations().expect("migrations");
        let account_id = store
            .upsert_account("acct-1", Some("alice"), Some("Alice"))
            .expect("upsert account");
        store
            .insert_token(
                account_id,
                "access-token",
                Some("refresh-token"),
                Some("2024-01-01T00:00:00Z"),
                Some("2999-01-01T00:00:00Z"),
                "{}",
            )
            .expect("insert token");

        run(
            super::super::DoctorCommand {
                command: DoctorSubcommand::Check,
            },
            &config,
            &store,
            OutputMode::Json,
        )
        .expect("doctor should pass");
    }

    #[test]
    fn doctor_fails_with_non_localhost_oauth_host() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("threads.db");
        let secret_path = dir.path().join("secret.txt");
        std::fs::write(&secret_path, "secret").expect("write secret");
        let mut config = sample_config(
            db_path.to_string_lossy().to_string(),
            secret_path.to_string_lossy().to_string(),
        );
        config.oauth.listen_host = "0.0.0.0".to_string();
        let store = Store::open(&config.storage.database_path).expect("store open");
        store.run_migrations().expect("migrations");

        let err = run(
            super::super::DoctorCommand {
                command: DoctorSubcommand::Check,
            },
            &config,
            &store,
            OutputMode::Json,
        )
        .expect_err("doctor should fail for non-localhost oauth");
        assert_eq!(err.category.as_code(), "CONFIG_ERROR");
        assert!(err.message.contains("oauth_localhost"));
    }
}
