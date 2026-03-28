use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::{CliError, ErrorCategory};

pub const DEFAULT_CONFIG_PATH: &str = "~/.config/threads-cli/config.toml";

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AppConfig {
    pub threads: ThreadsConfig,
    pub storage: StorageConfig,
    pub defaults: DefaultsConfig,
    pub oauth: OAuthConfig,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ThreadsConfig {
    pub app_id: String,
    pub app_secret_file: String,
    pub redirect_uri: String,
    pub user_id: String,
    #[serde(default = "default_base_url")]
    pub base_url: String,
    #[serde(default = "default_api_version")]
    pub version: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StorageConfig {
    pub database_path: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DefaultsConfig {
    #[serde(default = "default_link_mode")]
    pub link_mode: String,
    #[serde(default = "default_output_mode")]
    pub output: String,
    #[serde(default = "default_open_browser")]
    pub open_browser: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OAuthConfig {
    #[serde(default = "default_listen_host")]
    pub listen_host: String,
    #[serde(default = "default_listen_port")]
    pub listen_port: u16,
    #[serde(default = "default_state_ttl")]
    pub state_ttl_seconds: u64,
}

fn default_base_url() -> String {
    "https://graph.threads.net".to_string()
}
fn default_api_version() -> String {
    "v1.0".to_string()
}
fn default_link_mode() -> String {
    "reply".to_string()
}
fn default_output_mode() -> String {
    "human".to_string()
}
fn default_open_browser() -> bool {
    true
}
fn default_listen_host() -> String {
    "127.0.0.1".to_string()
}
fn default_listen_port() -> u16 {
    8788
}
fn default_state_ttl() -> u64 {
    600
}

impl AppConfig {
    pub fn load(config_path: Option<&str>) -> Result<Self, CliError> {
        let raw_path = config_path.unwrap_or(DEFAULT_CONFIG_PATH);
        let expanded = expand_path(raw_path)?;
        let contents = fs::read_to_string(&expanded).map_err(|e| {
            CliError::new(
                ErrorCategory::Config,
                format!("failed reading config file {}: {}", expanded.display(), e),
            )
        })?;
        let config: AppConfig = toml::from_str(&contents).map_err(|e| {
            CliError::new(ErrorCategory::Config, format!("invalid config TOML: {}", e))
        })?;
        config.validate()?;
        Ok(config)
    }

    pub fn effective_config_path(input_path: Option<&str>) -> Result<PathBuf, CliError> {
        expand_path(input_path.unwrap_or(DEFAULT_CONFIG_PATH))
    }

    pub fn read_app_secret(&self) -> Result<String, CliError> {
        let path = expand_path(&self.threads.app_secret_file)?;
        let secret = fs::read_to_string(&path).map_err(|e| {
            CliError::new(
                ErrorCategory::Config,
                format!("failed reading app secret file {}: {}", path.display(), e),
            )
        })?;
        let trimmed = secret.trim().to_string();
        if trimmed.is_empty() {
            return Err(CliError::new(
                ErrorCategory::Config,
                format!("app secret file {} is empty", path.display()),
            ));
        }
        Ok(trimmed)
    }

    pub fn validate(&self) -> Result<(), CliError> {
        if self.threads.app_id.trim().is_empty()
            || self.threads.app_secret_file.trim().is_empty()
            || self.threads.redirect_uri.trim().is_empty()
            || self.threads.user_id.trim().is_empty()
            || self.storage.database_path.trim().is_empty()
        {
            return Err(CliError::new(
                ErrorCategory::Config,
                "missing required config fields under [threads] or [storage]",
            ));
        }
        if self.oauth.listen_host != "127.0.0.1" && self.oauth.listen_host != "localhost" {
            return Err(CliError::new(
                ErrorCategory::Config,
                "oauth.listen_host must be localhost-only (127.0.0.1 or localhost)",
            ));
        }
        if self.defaults.link_mode != "reply" && self.defaults.link_mode != "attachment" {
            return Err(CliError::new(
                ErrorCategory::Config,
                "defaults.link_mode must be one of: reply, attachment",
            ));
        }
        if self.defaults.output != "human" && self.defaults.output != "json" {
            return Err(CliError::new(
                ErrorCategory::Config,
                "defaults.output must be one of: human, json",
            ));
        }
        if self.oauth.listen_port == 0 {
            return Err(CliError::new(
                ErrorCategory::Config,
                "oauth.listen_port must be non-zero",
            ));
        }
        Ok(())
    }

    pub fn redacted_for_display(&self) -> RedactedConfigView {
        RedactedConfigView {
            threads: RedactedThreadsConfig {
                app_id: self.threads.app_id.clone(),
                app_secret_file: self.threads.app_secret_file.clone(),
                app_secret_present: expand_path(&self.threads.app_secret_file)
                    .ok()
                    .map(|p| p.exists())
                    .unwrap_or(false),
                redirect_uri: self.threads.redirect_uri.clone(),
                user_id: self.threads.user_id.clone(),
                base_url: self.threads.base_url.clone(),
                version: self.threads.version.clone(),
            },
            storage: self.storage.clone(),
            defaults: self.defaults.clone(),
            oauth: self.oauth.clone(),
        }
    }
}

fn expand_path(input: &str) -> Result<PathBuf, CliError> {
    let expanded = shellexpand::full(input).map_err(|e| {
        CliError::new(
            ErrorCategory::Config,
            format!("failed expanding path '{}': {}", input, e),
        )
    })?;
    Ok(PathBuf::from(expanded.as_ref()))
}

#[derive(Debug, Clone, Serialize)]
pub struct RedactedConfigView {
    pub threads: RedactedThreadsConfig,
    pub storage: StorageConfig,
    pub defaults: DefaultsConfig,
    pub oauth: OAuthConfig,
}

#[derive(Debug, Clone, Serialize)]
pub struct RedactedThreadsConfig {
    pub app_id: String,
    pub app_secret_file: String,
    pub app_secret_present: bool,
    pub redirect_uri: String,
    pub user_id: String,
    pub base_url: String,
    pub version: String,
}
