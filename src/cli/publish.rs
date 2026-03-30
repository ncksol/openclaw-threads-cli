use clap::{Args, Subcommand};
use serde::Serialize;

use crate::client::{CreateContainerRequest, ThreadsClient};
use crate::config::AppConfig;
use crate::error::{CliError, ErrorCategory};
use crate::output::{print_success, OutputMode};
use crate::cli::validation;
use crate::store::{PersistPostInput, PublishAttemptInput, Store};

#[derive(Debug, Subcommand)]
pub enum PublishSubcommand {
    Post(PostArgs),
    Reply(ReplyArgs),
}

#[derive(Debug, Args)]
pub struct PostArgs {
    #[arg(long)]
    pub text: String,
    #[arg(long)]
    pub tag: Option<String>,
    #[arg(long)]
    pub link: Option<String>,
    #[arg(long, default_value = "reply")]
    pub link_mode: String,
}

#[derive(Debug, Args)]
pub struct ReplyArgs {
    #[arg(long)]
    pub reply_to: String,
    #[arg(long)]
    pub text: String,
}

#[derive(Debug, Serialize)]
struct StubData {
    implemented: bool,
    note: String,
    attempt_id: Option<i64>,
    attempt_uuid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    permalink: Option<String>,
}

pub fn run(
    command: super::PublishCommand,
    app: &AppConfig,
    store: &Store,
    output_mode: OutputMode,
) -> Result<(), CliError> {
    match command.command {
        PublishSubcommand::Post(args) => {
            validation::validate_post_text(&args.text)?;
            if let Some(tag) = &args.tag {
                validation::validate_topic_tag(tag)?;
            }
            if let Some(url) = &args.link {
                validation::validate_source_url(url)?;
            }
            if args.link_mode != "reply" && args.link_mode != "attachment" {
                return Err(CliError::new(
                    ErrorCategory::Validation,
                    "link-mode must be reply or attachment",
                ));
            }
            let (attempt_id, attempt_uuid) = store.create_publish_attempt(PublishAttemptInput {
                kind: "post".to_string(),
                text: args.text.clone(),
                reply_to_id: None,
                topic_tag: args.tag.clone(),
                source_url: args.link.clone(),
                source_link_mode: Some(args.link_mode.clone()),
                request_json: serde_json::json!({
                    "text": args.text,
                    "tag": args.tag,
                    "link": args.link,
                    "link_mode": args.link_mode
                })
                .to_string(),
            })?;
            let publish_result = run_publish_post_with_source_link(
                app,
                store,
                attempt_id,
                &args.text,
                args.tag.clone(),
                args.link.clone(),
                &args.link_mode,
            );
            let result_path = match publish_result {
                Ok(path) => path,
                Err(err) => return Err(err),
            };
            print_success(
                output_mode,
                "publish post",
                format!("Post published ({result_path})."),
                StubData {
                    implemented: true,
                    note: format!("Publish request completed ({result_path})."),
                    attempt_id: Some(attempt_id),
                    attempt_uuid: Some(attempt_uuid),
                    permalink: None,
                },
            );
            Ok(())
        }
        PublishSubcommand::Reply(args) => {
            validation::validate_reply_to(&args.reply_to)?;
            validation::validate_post_text(&args.text)?;
            let (attempt_id, attempt_uuid) = store.create_publish_attempt(PublishAttemptInput {
                kind: "reply".to_string(),
                text: args.text.clone(),
                reply_to_id: Some(args.reply_to.clone()),
                topic_tag: None,
                source_url: None,
                source_link_mode: None,
                request_json: serde_json::json!({
                    "reply_to": args.reply_to,
                    "text": args.text
                })
                .to_string(),
            })?;
            let publish_result = run_publish(
                app,
                store,
                attempt_id,
                CreateContainerRequest {
                    text: args.text.clone(),
                    media_type: "TEXT".to_string(),
                    reply_to_id: Some(args.reply_to.clone()),
                    topic_tag: None,
                    link_attachment: None,
                },
                PersistPostInput {
                    threads_post_id: String::new(),
                    parent_threads_post_id: Some(args.reply_to.clone()),
                    post_url: None,
                    text: args.text.clone(),
                    topic_tag: None,
                    source_url: None,
                    source_link_mode: None,
                    kind: "reply".to_string(),
                    published_at: None,
                    raw_json: String::new(),
                },
            );
            match publish_result {
                Ok(_) => {}
                Err(err) => return Err(err),
            }
            // Best-effort: fetch permalink for the newly published reply
            let permalink = fetch_reply_permalink(app, store);
            let human_text = match &permalink {
                Some(url) => format!("Reply published. {}", url),
                None => "Reply published.".to_string(),
            };
            print_success(
                output_mode,
                "publish reply",
                human_text,
                StubData {
                    implemented: true,
                    note: "Reply publish request completed.".to_string(),
                    attempt_id: Some(attempt_id),
                    attempt_uuid: Some(attempt_uuid),
                    permalink,
                },
            );
            Ok(())
        }
    }
}

fn run_publish(
    app: &AppConfig,
    store: &Store,
    attempt_id: i64,
    create_payload: CreateContainerRequest,
    mut post_input: PersistPostInput,
) -> Result<(), CliError> {
    let token = match store.latest_token()? {
        Some(token) => token,
        None => {
            let err = CliError::new(ErrorCategory::Auth, "not authenticated; run auth login first");
            store.mark_publish_attempt_failed(
                attempt_id,
                err.category.as_code(),
                &err.message,
            )?;
            return Err(err);
        }
    };
    let client = match ThreadsClient::from_config(app) {
        Ok(client) => client,
        Err(err) => {
            store.mark_publish_attempt_failed(
                attempt_id,
                err.category.as_code(),
                &err.message,
            )?;
            return Err(err);
        }
    };

    let publish_result =
        tokio::task::block_in_place(|| tokio::runtime::Handle::current().block_on(async {
            let create_response = client
                .create_publish_container(&token.access_token, &create_payload)
                .await?;
            let publish_response = client
                .publish_container(&token.access_token, &create_response.id)
                .await?;
            Ok::<_, CliError>((create_response, publish_response))
        }));

    match publish_result {
        Ok((create_response, publish_response)) => {
            let response_json = serde_json::json!({
                "create_container": create_response,
                "publish_container": publish_response,
            })
            .to_string();
            post_input.threads_post_id = publish_response.id.clone();
            post_input.raw_json = response_json.clone();
            if let Err(db_err) = store.persist_post(post_input) {
                store.mark_publish_attempt_ambiguous(
                    attempt_id,
                    ErrorCategory::Database.as_code(),
                    &db_err.message,
                    Some(&response_json),
                )?;
                return Err(build_ambiguous_publish_error(
                    attempt_id,
                    format!(
                        "publish completed remotely but failed to persist local post: {}",
                        db_err.message
                    ),
                ));
            }
            store.mark_publish_attempt_published(
                attempt_id,
                &publish_response.id,
                &response_json,
            )?;
            Ok(())
        }
        Err(err) => {
            let err_message = err.message.clone();
            let error_json = serde_json::json!({
                "error_code": err.category.as_code(),
                "error_message": &err_message,
            })
            .to_string();
            let ambiguous = is_ambiguous_publish_error(&err);
            if ambiguous {
                store.mark_publish_attempt_ambiguous(
                    attempt_id,
                    err.category.as_code(),
                    &err_message,
                    Some(&error_json),
                )?;
            } else {
                store.mark_publish_attempt_failed(
                    attempt_id,
                    err.category.as_code(),
                    &err_message,
                )?;
            }
            if ambiguous {
                Err(build_ambiguous_publish_error(attempt_id, err_message))
            } else {
                Err(err)
            }
        }
    }
}

fn run_publish_post_with_source_link(
    app: &AppConfig,
    store: &Store,
    attempt_id: i64,
    text: &str,
    topic_tag: Option<String>,
    source_url: Option<String>,
    link_mode: &str,
) -> Result<String, CliError> {
    let token = match store.latest_token()? {
        Some(token) => token,
        None => {
            let err = CliError::new(ErrorCategory::Auth, "not authenticated; run auth login first");
            store.mark_publish_attempt_failed(
                attempt_id,
                err.category.as_code(),
                &err.message,
            )?;
            return Err(err);
        }
    };
    let client = match ThreadsClient::from_config(app) {
        Ok(client) => client,
        Err(err) => {
            store.mark_publish_attempt_failed(
                attempt_id,
                err.category.as_code(),
                &err.message,
            )?;
            return Err(err);
        }
    };
    let source_link_mode = Some(link_mode.to_string());

    let create_payload = CreateContainerRequest {
        text: text.to_string(),
        media_type: "TEXT".to_string(),
        reply_to_id: None,
        topic_tag: topic_tag.clone(),
        link_attachment: if link_mode == "attachment" {
            source_url.clone()
        } else {
            None
        },
    };

    let publish_result =
        tokio::task::block_in_place(|| tokio::runtime::Handle::current().block_on(async {
            let create_response = client
                .create_publish_container(&token.access_token, &create_payload)
                .await?;

            ensure_attachment_supported(link_mode, &source_url)?;

            let publish_response = client
                .publish_container(&token.access_token, &create_response.id)
                .await?;

            if link_mode == "reply" && source_url.is_some() {
                let source_payload = CreateContainerRequest {
                    text: source_url.clone().expect("source_url checked as some"),
                    media_type: "TEXT".to_string(),
                    reply_to_id: Some(publish_response.id.clone()),
                    topic_tag: None,
                    link_attachment: None,
                };
                let source_create_response = client
                    .create_publish_container(&token.access_token, &source_payload)
                    .await?;
                let source_publish_response = client
                    .publish_container(&token.access_token, &source_create_response.id)
                    .await?;
                Ok::<_, CliError>((
                    create_response,
                    publish_response,
                    Some((source_create_response, source_publish_response)),
                ))
            } else {
                Ok::<_, CliError>((create_response, publish_response, None))
            }
        }));

    match publish_result {
        Ok((create_response, publish_response, source_result)) => {
            let mut response_value = serde_json::json!({
                "mode": link_mode,
                "result_path": if source_result.is_some() { "reply-chained" } else { "single-post" },
                "create_container": create_response,
                "publish_container": publish_response,
            });

            let mut post_input = PersistPostInput {
                threads_post_id: publish_response.id.clone(),
                parent_threads_post_id: None,
                post_url: None,
                text: text.to_string(),
                topic_tag: topic_tag.clone(),
                source_url: source_url.clone(),
                source_link_mode: source_link_mode.clone(),
                kind: "post".to_string(),
                published_at: None,
                raw_json: String::new(),
            };

            if let Some((source_create, source_publish)) = source_result {
                response_value["source_reply"] = serde_json::json!({
                    "create_container": source_create,
                    "publish_container": source_publish,
                });
                if let Err(db_err) = persist_source_link_reply_post(
                    store,
                    &source_publish.id,
                    &publish_response.id,
                    source_url.as_deref().unwrap_or_default(),
                    source_link_mode.clone(),
                    &source_create,
                    &source_publish,
                ) {
                    let response_json = response_value.to_string();
                    store.mark_publish_attempt_ambiguous(
                        attempt_id,
                        ErrorCategory::Database.as_code(),
                        &db_err.message,
                        Some(&response_json),
                    )?;
                    return Err(build_ambiguous_publish_error(
                        attempt_id,
                        format!(
                            "publish completed remotely but failed to persist source link reply: {}",
                            db_err.message
                        ),
                    ));
                }
            }

            let response_json = response_value.to_string();
            post_input.raw_json = response_json.clone();
            if let Err(db_err) = store.persist_post(post_input) {
                store.mark_publish_attempt_ambiguous(
                    attempt_id,
                    ErrorCategory::Database.as_code(),
                    &db_err.message,
                    Some(&response_json),
                )?;
                return Err(build_ambiguous_publish_error(
                    attempt_id,
                    format!(
                        "publish completed remotely but failed to persist local post: {}",
                        db_err.message
                    ),
                ));
            }
            store.mark_publish_attempt_published(
                attempt_id,
                &publish_response.id,
                &response_json,
            )?;
            Ok(if source_url.is_some() {
                if link_mode == "reply" {
                    "mode=reply path=reply-chained".to_string()
                } else {
                    "mode=attachment path=main-post-attachment".to_string()
                }
            } else {
                format!("mode={} path=single-post", link_mode)
            })
        }
        Err(err) => {
            let err_message = err.message.clone();
            let error_json = serde_json::json!({
                "mode": link_mode,
                "result_path": if link_mode == "reply" { "reply-chained" } else { "main-post-attachment" },
                "error_code": err.category.as_code(),
                "error_message": &err_message,
            })
            .to_string();
            let ambiguous = is_ambiguous_publish_error(&err);
            if link_mode == "attachment" && err.category.as_code() == "API_ERROR" {
                store.mark_publish_attempt_failed_with_response(
                    attempt_id,
                    err.category.as_code(),
                    &err_message,
                    &error_json,
                )?;
            } else if ambiguous {
                store.mark_publish_attempt_ambiguous(
                    attempt_id,
                    err.category.as_code(),
                    &err_message,
                    Some(&error_json),
                )?;
            } else {
                store.mark_publish_attempt_failed(
                    attempt_id,
                    err.category.as_code(),
                    &err_message,
                )?;
            }
            if ambiguous {
                Err(build_ambiguous_publish_error(attempt_id, err_message))
            } else {
                Err(err)
            }
        }
    }
}

fn build_ambiguous_publish_error(attempt_id: i64, detail: String) -> CliError {
    CliError::new(
        ErrorCategory::AmbiguousPublish,
        format!(
            "{} Recovery: inspect via `threads-cli attempts list --limit 20` and avoid re-posting blindly; confirm remotely before retrying (attempt_id={}).",
            detail, attempt_id
        ),
    )
}

fn is_ambiguous_publish_error(err: &CliError) -> bool {
    matches!(err.category, ErrorCategory::Network | ErrorCategory::RateLimit)
        || (matches!(err.category, ErrorCategory::Api) && err.message.contains("HTTP 5"))
}

fn ensure_attachment_supported(
    link_mode: &str,
    source_url: &Option<String>,
) -> Result<(), CliError> {
    if link_mode == "attachment" && source_url.is_some() {
        return Err(CliError::new(
            ErrorCategory::Api,
            "attachment mode requested but API payload/response does not support link_attachment",
        ));
    }
    Ok(())
}

fn fetch_reply_permalink(app: &AppConfig, store: &Store) -> Option<String> {
    let token = store.latest_token().ok()??;
    let client = ThreadsClient::from_config(app).ok()?;
    // Get the most recently published attempt to find the threads_post_id
    let attempts = store.list_attempts(1).ok()?;
    let latest = attempts.first()?;
    let post_id = latest.threads_post_id.as_ref()?;
    let permalink = tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current().block_on(async {
            client
                .fetch_post_details(&token.access_token, post_id)
                .await
                .ok()
                .and_then(|details| details.permalink)
        })
    });
    permalink
}

fn persist_source_link_reply_post(
    store: &Store,
    source_reply_id: &str,
    parent_post_id: &str,
    source_text: &str,
    source_link_mode: Option<String>,
    source_create: &crate::client::CreateContainerResponse,
    source_publish: &crate::client::PublishContainerResponse,
) -> Result<(), CliError> {
    let source_response_json = serde_json::json!({
        "mode": "reply",
        "result_path": "reply-chained",
        "is_source_link_reply": true,
        "source_parent_threads_post_id": parent_post_id,
        "create_container": source_create,
        "publish_container": source_publish,
    })
    .to_string();
    let source_post_input = PersistPostInput {
        threads_post_id: source_reply_id.to_string(),
        parent_threads_post_id: Some(parent_post_id.to_string()),
        post_url: None,
        text: source_text.to_string(),
        topic_tag: None,
        source_url: Some(source_text.to_string()),
        source_link_mode,
        kind: "reply".to_string(),
        published_at: None,
        raw_json: source_response_json,
    };
    store.persist_post(source_post_input)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};
    use std::thread;
    use rusqlite::OptionalExtension;
    use serde_json::json;

    #[derive(Debug, Clone)]
    struct RecordedRequest {
        path: String,
        headers: String,
        body: String,
    }

    fn test_config(db_path: &str) -> AppConfig {
        AppConfig {
            threads: crate::config::ThreadsConfig {
                app_id: "app-id".to_string(),
                app_secret_file: db_path.replace("threads.db", "secret.txt"),
                redirect_uri: "http://127.0.0.1:8788/callback".to_string(),
                user_id: "user-id".to_string(),
                base_url: "http://127.0.0.1:0".to_string(),
                version: "v1.0".to_string(),
            },
            storage: crate::config::StorageConfig {
                database_path: db_path.to_string(),
            },
            defaults: crate::config::DefaultsConfig {
                link_mode: "reply".to_string(),
                output: "human".to_string(),
                open_browser: true,
            },
            oauth: crate::config::OAuthConfig {
                listen_host: "127.0.0.1".to_string(),
                listen_port: 8788,
                state_ttl_seconds: 600,
            },
        }
    }

    fn write_secret_file(app: &AppConfig) {
        std::fs::write(&app.threads.app_secret_file, "test-secret").expect("write secret file");
    }

    fn add_token(store: &Store) {
        let account_id = store
            .upsert_account("user-id", Some("tester"), Some("Tester"))
            .expect("upsert account");
        store
            .insert_token(
                account_id,
                "test-token",
                None,
                Some("2024-01-01T00:00:00Z"),
                None,
                "{}",
            )
            .expect("insert token");
    }

    fn spawn_mock_server(
        expected_paths: usize,
        create_responses: Vec<serde_json::Value>,
        publish_responses: Vec<serde_json::Value>,
    ) -> String {
        spawn_recording_mock_server(expected_paths, create_responses, publish_responses).0
    }

    fn spawn_recording_mock_server(
        expected_paths: usize,
        create_responses: Vec<serde_json::Value>,
        publish_responses: Vec<serde_json::Value>,
    ) -> (String, Arc<Mutex<Vec<RecordedRequest>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind server");
        let addr = listener.local_addr().expect("local addr");
        let recorded = Arc::new(Mutex::new(Vec::new()));
        let recorded_clone = Arc::clone(&recorded);
        
        thread::spawn(move || {
            let mut create_idx = 0usize;
            let mut publish_idx = 0usize;
            for _ in 0..expected_paths {
                let (mut stream, _) = listener.accept().expect("accept");
                let mut request = Vec::new();
                let mut temp = [0u8; 4096];
                
                // Read until we have the full headers
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
                
                // Find header boundary in byte-space
                let header_end = request.windows(4)
                    .position(|w| w == b"\r\n\r\n")
                    .unwrap_or(request.len());
                let headers_bytes = &request[..header_end];
                let headers_str = String::from_utf8_lossy(headers_bytes);
                
                // Parse Content-Length to read body
                let content_length: usize = headers_str
                    .lines()
                    .find(|line| line.to_lowercase().starts_with("content-length:"))
                    .and_then(|line| line.split(':').nth(1))
                    .and_then(|val| val.trim().parse().ok())
                    .unwrap_or(0);
                
                // Read body if present
                let body_start = header_end + 4;
                let mut body_bytes = if body_start < request.len() {
                    request[body_start..].to_vec()
                } else {
                    Vec::new()
                };
                
                while body_bytes.len() < content_length {
                    let n = stream.read(&mut temp).expect("read body");
                    if n == 0 {
                        break;
                    }
                    body_bytes.extend_from_slice(&temp[..n]);
                }
                
                let body = String::from_utf8_lossy(&body_bytes[..content_length.min(body_bytes.len())]).to_string();
                
                let req_text = String::from_utf8_lossy(&request);
                let first_line = req_text.lines().next().unwrap_or_default();
                let path = first_line.split_whitespace().nth(1).unwrap_or("/").to_string();
                
                // Record this request
                recorded_clone.lock().expect("lock recorded requests").push(RecordedRequest {
                    path: path.clone(),
                    headers: headers_str.to_string(),
                    body: body.clone(),
                });
                
                // Return canned response
                let (status, response_body) = if path.contains("/threads_publish") {
                    let body = publish_responses[publish_idx].to_string();
                    publish_idx += 1;
                    ("200 OK", body)
                } else if path.contains("/threads?") {
                    let body = create_responses[create_idx].to_string();
                    create_idx += 1;
                    ("200 OK", body)
                } else {
                    ("404 Not Found", json!({"error":{"message":"not found"}}).to_string())
                };
                
                let response = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response_body}",
                    response_body.len()
                );
                stream.write_all(response.as_bytes()).expect("write response");
                stream.flush().expect("flush");
            }
        });
        
        (format!("http://{}", addr), recorded)
    }

    #[test]
    fn post_accepts_attachment_mode_and_records_attempt_metadata() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("threads.db");
        let db_path_str = db_path.to_str().expect("db path");
        let app = test_config(db_path_str);
        let store = Store::open(db_path_str).expect("open store");
        store.run_migrations().expect("run migrations");

        let command = super::super::PublishCommand {
            command: PublishSubcommand::Post(PostArgs {
                text: "hello from attachment mode".to_string(),
                tag: Some("demo".to_string()),
                link: Some("https://example.com/source".to_string()),
                link_mode: "attachment".to_string(),
            }),
        };

        let err =
            run(command, &app, &store, OutputMode::Human).expect_err("publish post should fail without auth");
        assert_eq!(err.category.as_code(), "AUTH_ERROR");

        let conn = store.connection().expect("connection");
        let row: Option<(Option<String>, Option<String>, String, String, Option<String>)> = conn
            .query_row(
                "SELECT source_link_mode, source_url, request_json, status, error_code
                 FROM publish_attempts
                 ORDER BY id DESC
                 LIMIT 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
            )
            .optional()
            .expect("query row");

        let (source_link_mode, source_url, request_json, status, error_code) = row.expect("attempt row");
        assert_eq!(source_link_mode.as_deref(), Some("attachment"));
        assert_eq!(source_url.as_deref(), Some("https://example.com/source"));
        assert_eq!(status, "failed");
        assert_eq!(error_code.as_deref(), Some("AUTH_ERROR"));

        let parsed: serde_json::Value =
            serde_json::from_str(&request_json).expect("request_json must be valid json");
        assert_eq!(parsed["link_mode"], "attachment");
        assert_eq!(parsed["link"], "https://example.com/source");
    }

    #[test]
    fn post_rejects_unknown_link_mode() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("threads.db");
        let db_path_str = db_path.to_str().expect("db path");
        let app = test_config(db_path_str);
        let store = Store::open(db_path_str).expect("open store");
        store.run_migrations().expect("run migrations");

        let command = super::super::PublishCommand {
            command: PublishSubcommand::Post(PostArgs {
                text: "hello".to_string(),
                tag: None,
                link: Some("https://example.com".to_string()),
                link_mode: "unknown".to_string(),
            }),
        };

        let err = run(command, &app, &store, OutputMode::Human)
            .expect_err("publish post should reject unknown link mode");
        assert_eq!(err.category.as_code(), "VALIDATION_ERROR");
        assert_eq!(err.message, "link-mode must be reply or attachment");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn post_reply_mode_publishes_source_link_reply_and_persists_both() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("threads.db");
        let db_path_str = db_path.to_str().expect("db path");
        let mut app = test_config(db_path_str);
        write_secret_file(&app);
        let base_url = spawn_mock_server(
            4,
            vec![json!({"id":"create_main"}), json!({"id":"create_source"})],
            vec![json!({"id":"post_main"}), json!({"id":"post_source"})],
        );
        app.threads.base_url = base_url;
        let store = Store::open(db_path_str).expect("open store");
        store.run_migrations().expect("run migrations");
        add_token(&store);

        let command = super::super::PublishCommand {
            command: PublishSubcommand::Post(PostArgs {
                text: "hello".to_string(),
                tag: Some("demo".to_string()),
                link: Some("https://example.com/source".to_string()),
                link_mode: "reply".to_string(),
            }),
        };

        run(command, &app, &store, OutputMode::Json).expect("publish should succeed");

        let conn = store.connection().expect("connection");
        let attempts_row: (String, Option<String>) = conn
            .query_row(
                "SELECT status, response_json FROM publish_attempts ORDER BY id DESC LIMIT 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("attempt row");
        assert_eq!(attempts_row.0, "published");
        let response_json = attempts_row.1.expect("response_json");
        let response: serde_json::Value = serde_json::from_str(&response_json).expect("parse response");
        assert_eq!(response["mode"], "reply");
        assert_eq!(response["result_path"], "reply-chained");
        assert_eq!(response["publish_container"]["id"], "post_main");
        assert_eq!(response["source_reply"]["publish_container"]["id"], "post_source");

        let mut stmt = conn
            .prepare(
                "SELECT threads_post_id, parent_threads_post_id, kind, source_link_mode, source_url
                 FROM posts ORDER BY id ASC",
            )
            .expect("prepare");
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                ))
            })
            .expect("query rows")
            .collect::<Result<Vec<_>, _>>()
            .expect("collect rows");
        assert_eq!(rows.len(), 2);
        let main = rows
            .iter()
            .find(|r| r.0 == "post_main")
            .expect("main post row must exist");
        assert_eq!(main.1, None);
        assert_eq!(main.2, "post");
        assert_eq!(main.3.as_deref(), Some("reply"));

        let source = rows
            .iter()
            .find(|r| r.0 == "post_source")
            .expect("source reply row must exist");
        assert_eq!(source.1.as_deref(), Some("post_main"));
        assert_eq!(source.2, "reply");
        assert_eq!(source.3.as_deref(), Some("reply"));
        assert_eq!(source.4.as_deref(), Some("https://example.com/source"));

        let source_row = store
            .get_post_by_threads_post_id("post_source")
            .expect("query source post by threads id")
            .expect("source row by id");
        assert_eq!(source_row.parent_threads_post_id.as_deref(), Some("post_main"));
        assert_eq!(source_row.kind, "reply");
        let parent_rows = store
            .list_posts_by_parent("post_main", 10)
            .expect("list posts by parent");
        assert_eq!(parent_rows.len(), 1);
        assert_eq!(parent_rows[0].threads_post_id, "post_source");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn post_attachment_mode_fails_if_unsupported_and_records_clear_failure() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("threads.db");
        let db_path_str = db_path.to_str().expect("db path");
        let mut app = test_config(db_path_str);
        write_secret_file(&app);
        let base_url = spawn_mock_server(1, vec![json!({"id":"create_main"})], vec![]);
        app.threads.base_url = base_url;
        let store = Store::open(db_path_str).expect("open store");
        store.run_migrations().expect("run migrations");
        add_token(&store);

        let command = super::super::PublishCommand {
            command: PublishSubcommand::Post(PostArgs {
                text: "hello attachment".to_string(),
                tag: None,
                link: Some("https://example.com/source".to_string()),
                link_mode: "attachment".to_string(),
            }),
        };

        let err = run(command, &app, &store, OutputMode::Json).expect_err("must fail clearly");
        assert_eq!(err.category.as_code(), "API_ERROR");
        assert_eq!(
            err.message,
            "attachment mode requested but API payload/response does not support link_attachment"
        );

        let conn = store.connection().expect("connection");
        let row: (String, Option<String>, Option<String>, Option<String>) = conn
            .query_row(
                "SELECT status, error_code, error_message, response_json
                 FROM publish_attempts
                 ORDER BY id DESC
                 LIMIT 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .expect("attempt row");
        assert_eq!(row.0, "failed");
        assert_eq!(row.1.as_deref(), Some("API_ERROR"));
        assert_eq!(
            row.2.as_deref(),
            Some("attachment mode requested but API payload/response does not support link_attachment")
        );
        let response_json = row.3.expect("response json");
        let response: serde_json::Value = serde_json::from_str(&response_json).expect("parse");
        assert_eq!(response["mode"], "attachment");
        assert_eq!(response["result_path"], "main-post-attachment");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn post_reply_network_failure_marks_attempt_ambiguous_with_recovery_guidance() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("threads.db");
        let db_path_str = db_path.to_str().expect("db path");
        let mut app = test_config(db_path_str);
        write_secret_file(&app);
        let base_url = spawn_mock_server(1, vec![json!({"id":"create_main"})], vec![]);
        app.threads.base_url = base_url;
        let store = Store::open(db_path_str).expect("open store");
        store.run_migrations().expect("run migrations");
        add_token(&store);

        let command = super::super::PublishCommand {
            command: PublishSubcommand::Reply(ReplyArgs {
                reply_to: "parent_1".to_string(),
                text: "hello reply".to_string(),
            }),
        };

        let err = run(command, &app, &store, OutputMode::Json).expect_err("publish should be ambiguous");
        assert_eq!(err.category.as_code(), "AMBIGUOUS_PUBLISH_ERROR");
        assert!(err.message.contains("Recovery: inspect via `threads-cli attempts list --limit 20`"));

        let conn = store.connection().expect("connection");
        let row: (String, Option<String>, Option<String>) = conn
            .query_row(
                "SELECT status, error_code, error_message
                 FROM publish_attempts
                 ORDER BY id DESC
                 LIMIT 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("attempt row");
        assert_eq!(row.0, "ambiguous");
        assert_eq!(row.1.as_deref(), Some("NETWORK_ERROR"));
        assert!(row
            .2
            .as_deref()
            .expect("error message")
            .contains("publish container failed: network error"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn reply_publish_uses_form_encoded_container_request() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("threads.db");
        let db_path_str = db_path.to_str().expect("db path");
        let mut app = test_config(db_path_str);
        write_secret_file(&app);
        
        let (base_url, recorded) = spawn_recording_mock_server(
            2,
            vec![json!({"id":"create_reply"})],
            vec![json!({"id":"publish_reply"})],
        );
        app.threads.base_url = base_url;
        
        let store = Store::open(db_path_str).expect("open store");
        store.run_migrations().expect("run migrations");
        add_token(&store);
        
        let command = super::super::PublishCommand {
            command: PublishSubcommand::Reply(ReplyArgs {
                reply_to: "parent_1".to_string(),
                text: "reply-body".to_string(),
            }),
        };
        
        run(command, &app, &store, OutputMode::Json).expect("publish should succeed");
        
        let requests = recorded.lock().expect("lock recorded requests");
        let create_req = requests.iter().find(|r| r.path.contains("/threads?")).expect("create request");
        
        // Assert path contains access_token
        assert!(create_req.path.contains("access_token=test-token"), "path should contain access_token=test-token");
        
        // Assert Content-Type is form-encoded
        assert!(
            create_req.headers.contains("Content-Type: application/x-www-form-urlencoded") ||
            create_req.headers.contains("content-type: application/x-www-form-urlencoded"),
            "headers should contain Content-Type: application/x-www-form-urlencoded, got: {}",
            create_req.headers
        );
        
        // Assert body contains correct fields
        assert!(create_req.body.contains("text=reply-body"), "body should contain text=reply-body, got: {}", create_req.body);
        assert!(create_req.body.contains("media_type=TEXT"), "body should contain media_type=TEXT, got: {}", create_req.body);
        assert!(create_req.body.contains("reply_to_id=parent_1"), "body should contain reply_to_id=parent_1, got: {}", create_req.body);
        
        // Assert optional fields are omitted
        assert!(!create_req.body.contains("topic_tag="), "body should not contain topic_tag=");
        assert!(!create_req.body.contains("link_attachment="), "body should not contain link_attachment=");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn post_publish_uses_form_encoded_container_request_and_omits_optional_fields() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("threads.db");
        let db_path_str = db_path.to_str().expect("db path");
        let mut app = test_config(db_path_str);
        write_secret_file(&app);
        
        let (base_url, recorded) = spawn_recording_mock_server(
            2,
            vec![json!({"id":"create_post"})],
            vec![json!({"id":"publish_post"})],
        );
        app.threads.base_url = base_url;
        
        let store = Store::open(db_path_str).expect("open store");
        store.run_migrations().expect("run migrations");
        add_token(&store);
        
        let command = super::super::PublishCommand {
            command: PublishSubcommand::Post(PostArgs {
                text: "post-body".to_string(),
                tag: None,
                link: None,
                link_mode: "reply".to_string(),
            }),
        };
        
        run(command, &app, &store, OutputMode::Json).expect("publish should succeed");
        
        let requests = recorded.lock().expect("lock recorded requests");
        let create_req = requests.iter().find(|r| r.path.contains("/threads?")).expect("create request");
        
        // Assert path contains access_token
        assert!(create_req.path.contains("access_token=test-token"), "path should contain access_token=test-token");
        
        // Assert Content-Type is form-encoded
        assert!(
            create_req.headers.contains("Content-Type: application/x-www-form-urlencoded") ||
            create_req.headers.contains("content-type: application/x-www-form-urlencoded"),
            "headers should contain Content-Type: application/x-www-form-urlencoded, got: {}",
            create_req.headers
        );
        
        // Assert body contains correct fields
        assert!(create_req.body.contains("text=post-body"), "body should contain text=post-body, got: {}", create_req.body);
        assert!(create_req.body.contains("media_type=TEXT"), "body should contain media_type=TEXT, got: {}", create_req.body);
        
        // Assert optional fields are omitted
        assert!(!create_req.body.contains("reply_to_id="), "body should not contain reply_to_id=");
        assert!(!create_req.body.contains("topic_tag="), "body should not contain topic_tag=");
        assert!(!create_req.body.contains("link_attachment="), "body should not contain link_attachment=");
    }
}
