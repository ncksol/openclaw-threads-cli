use std::borrow::Cow;
use std::time::Duration;

use reqwest::{Client, StatusCode};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::config::AppConfig;
use crate::error::{CliError, ErrorCategory};

#[derive(Clone)]
pub struct ThreadsClient {
    http: Client,
    base_url: String,
    api_version: String,
    user_id: String,
    app_id: String,
    app_secret: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct HealthResult {
    pub base_url: String,
    pub api_version: String,
    pub http_ready: bool,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct OAuthTokenResponse {
    pub access_token: String,
    #[serde(default)]
    pub token_type: Option<String>,
    #[serde(default)]
    pub expires_in: Option<i64>,
    #[serde(default)]
    pub refresh_token: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct AccountIdentity {
    pub id: String,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateContainerRequest {
    pub text: String,
    #[serde(rename = "media_type")]
    pub media_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply_to_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub topic_tag: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link_attachment: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct CreateContainerResponse {
    pub id: String,
    #[serde(default)]
    pub link_attachment: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct PublishContainerResponse {
    pub id: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct PostDetails {
    pub id: String,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub permalink: Option<String>,
    #[serde(default)]
    pub timestamp: Option<String>,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub shortcode: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct PostInsightsResponse {
    #[serde(default)]
    pub data: Vec<InsightMetric>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct InsightMetric {
    pub name: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub values: Vec<InsightValue>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct InsightValue {
    #[serde(default)]
    pub value: Option<i64>,
    #[serde(default)]
    pub end_time: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct RepliesResponse {
    #[serde(default)]
    pub data: Vec<ReplyItem>,
    #[serde(default)]
    pub paging: Option<Paging>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ReplyItem {
    pub id: String,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub timestamp: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Paging {
    #[serde(default)]
    pub cursors: Option<Cursors>,
    #[serde(default)]
    pub next: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Cursors {
    #[serde(default)]
    pub before: Option<String>,
    #[serde(default)]
    pub after: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ApiErrorEnvelope {
    error: ApiErrorPayload,
}

#[derive(Debug, Deserialize)]
struct ApiErrorPayload {
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    code: Option<i64>,
}

pub fn map_network_error(error: reqwest::Error, operation: &str) -> CliError {
    CliError::new(
        ErrorCategory::Network,
        format!("{} failed: network error: {}", operation, error),
    )
}

pub fn map_api_error(status: StatusCode, response_body: &str, operation: &str) -> CliError {
    let category = match status {
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => ErrorCategory::Auth,
        StatusCode::TOO_MANY_REQUESTS => ErrorCategory::RateLimit,
        _ => ErrorCategory::Api,
    };
    let api_message = serde_json::from_str::<ApiErrorEnvelope>(response_body)
        .ok()
        .and_then(|payload| payload.error.message)
        .filter(|msg| !msg.trim().is_empty());
    let api_code = serde_json::from_str::<ApiErrorEnvelope>(response_body)
        .ok()
        .and_then(|payload| payload.error.code);
    let detail = match (api_message, api_code) {
        (Some(message), Some(code)) => Cow::Owned(format!("{} (code {})", message, code)),
        (Some(message), None) => Cow::Owned(message),
        (None, Some(code)) => Cow::Owned(format!("API error code {}", code)),
        (None, None) => Cow::Borrowed("no API error payload"),
    };
    CliError::new(
        category,
        format!(
            "{} failed: HTTP {}: {}",
            operation,
            status.as_u16(),
            detail.as_ref()
        ),
    )
}

impl ThreadsClient {
    pub fn from_config(config: &AppConfig) -> Result<Self, CliError> {
        let http = Client::builder()
            .user_agent("threads-cli/0.1.0")
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| CliError::new(ErrorCategory::Network, format!("http client init error: {}", e)))?;

        Ok(Self {
            http,
            base_url: config.threads.base_url.clone(),
            api_version: config.threads.version.clone(),
            user_id: config.threads.user_id.clone(),
            app_id: config.threads.app_id.clone(),
            app_secret: config.read_app_secret()?,
        })
    }

    pub fn health(&self) -> HealthResult {
        HealthResult {
            base_url: self.base_url.clone(),
            api_version: self.api_version.clone(),
            http_ready: true,
        }
    }

    pub async fn exchange_oauth_token(&self, code: &str, redirect_uri: &str) -> Result<OAuthTokenResponse, CliError> {
        let path = self.versioned_path("/oauth/access_token");
        let request = self.http.post(path).form(&[
            ("client_id", self.app_id.as_str()),
            ("client_secret", self.app_secret.as_str()),
            ("grant_type", "authorization_code"),
            ("redirect_uri", redirect_uri),
            ("code", code),
        ]);
        self.execute_json(request, "oauth token exchange").await
    }

    pub async fn refresh_oauth_token(&self, refresh_token: &str) -> Result<OAuthTokenResponse, CliError> {
        let path = self.versioned_path("/oauth/access_token");
        let request = self.http.post(path).form(&[
            ("client_id", self.app_id.as_str()),
            ("client_secret", self.app_secret.as_str()),
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
        ]);
        self.execute_json(request, "oauth token refresh").await
    }

    pub async fn exchange_long_lived_token(
        &self,
        access_token: &str,
    ) -> Result<OAuthTokenResponse, CliError> {
        let path = self.versioned_path("/access_token");
        let request = self.http.get(path).query(&[
            ("grant_type", "th_exchange_token"),
            ("client_secret", self.app_secret.as_str()),
            ("access_token", access_token),
        ]);
        self.execute_json(request, "oauth long-lived token exchange").await
    }

    pub async fn fetch_account_identity(&self, access_token: &str) -> Result<AccountIdentity, CliError> {
        let path = self.versioned_path("/me");
        let request = self
            .http
            .get(path)
            .query(&[("fields", "id,username,name"), ("access_token", access_token)]);
        self.execute_json(request, "account identity fetch").await
    }

    pub async fn create_publish_container(
        &self,
        access_token: &str,
        payload: &CreateContainerRequest,
    ) -> Result<CreateContainerResponse, CliError> {
        let path = self.versioned_path(&format!("/{}/threads", self.user_id));
        let request = self
            .http
            .post(path)
            .query(&[("access_token", access_token)])
            .json(payload);
        self.execute_json(request, "create publish container").await
    }

    pub async fn publish_container(
        &self,
        access_token: &str,
        creation_id: &str,
    ) -> Result<PublishContainerResponse, CliError> {
        let path = self.versioned_path(&format!("/{}/threads_publish", self.user_id));
        let request = self.http.post(path).query(&[
            ("access_token", access_token),
            ("creation_id", creation_id),
        ]);
        self.execute_json(request, "publish container").await
    }

    pub async fn fetch_post_details(&self, access_token: &str, post_id: &str) -> Result<PostDetails, CliError> {
        let path = self.versioned_path(&format!("/{}", post_id));
        let request = self.http.get(path).query(&[
            ("fields", "id,text,permalink,timestamp,username,shortcode"),
            ("access_token", access_token),
        ]);
        self.execute_json(request, "post details fetch").await
    }

    pub async fn fetch_post_insights(
        &self,
        access_token: &str,
        post_id: &str,
    ) -> Result<PostInsightsResponse, CliError> {
        let path = self.versioned_path(&format!("/{}/insights", post_id));
        let request = self.http.get(path).query(&[
            ("metric", "views,likes,replies,reposts,quotes,shares"),
            ("access_token", access_token),
        ]);
        self.execute_json(request, "post insights fetch").await
    }

    pub async fn fetch_replies(
        &self,
        access_token: &str,
        post_id: &str,
        limit: Option<u32>,
        after: Option<&str>,
    ) -> Result<RepliesResponse, CliError> {
        let path = self.versioned_path(&format!("/{}/replies", post_id));
        let mut query = vec![
            ("fields".to_string(), "id,text,username,timestamp".to_string()),
            ("access_token".to_string(), access_token.to_string()),
        ];
        if let Some(limit) = limit {
            query.push(("limit".to_string(), limit.to_string()));
        }
        if let Some(after) = after {
            query.push(("after".to_string(), after.to_string()));
        }
        let request = self.http.get(path).query(&query);
        self.execute_json(request, "post replies fetch").await
    }

    fn versioned_path(&self, resource: &str) -> String {
        format!(
            "{}/{}/{}",
            self.base_url.trim_end_matches('/'),
            self.api_version.trim_start_matches('/'),
            resource.trim_start_matches('/')
        )
    }

    async fn execute_json<T: DeserializeOwned>(
        &self,
        request: reqwest::RequestBuilder,
        operation: &str,
    ) -> Result<T, CliError> {
        let response = request
            .send()
            .await
            .map_err(|e| map_network_error(e, operation))?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(map_api_error(status, &body, operation));
        }
        response.json::<T>().await.map_err(|e| {
            CliError::new(
                ErrorCategory::Api,
                format!("{} failed: response parse error: {}", operation, e),
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_auth_error_category() {
        let err = map_api_error(
            StatusCode::UNAUTHORIZED,
            r#"{"error":{"message":"Invalid OAuth 2.0 Access Token","code":190}}"#,
            "account identity fetch",
        );
        assert_eq!(err.category.as_code(), "AUTH_ERROR");
        assert!(err.message.contains("code 190"));
    }

    #[test]
    fn maps_rate_limit_error_category() {
        let err = map_api_error(
            StatusCode::TOO_MANY_REQUESTS,
            r#"{"error":{"message":"Application request limit reached","code":4}}"#,
            "post insights fetch",
        );
        assert_eq!(err.category.as_code(), "RATE_LIMIT_ERROR");
    }

    #[test]
    fn serializes_container_payload_contract() {
        let payload = CreateContainerRequest {
            text: "hello".to_string(),
            media_type: "TEXT".to_string(),
            reply_to_id: Some("123".to_string()),
            topic_tag: Some("rust".to_string()),
            link_attachment: Some("https://example.com".to_string()),
        };
        let value = serde_json::to_value(payload).expect("serialize payload");
        assert_eq!(value["text"], "hello");
        assert_eq!(value["media_type"], "TEXT");
        assert_eq!(value["reply_to_id"], "123");
        assert_eq!(value["topic_tag"], "rust");
        assert_eq!(value["link_attachment"], "https://example.com");
    }
}
