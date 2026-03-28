use clap::{Args, Subcommand};
use serde::Serialize;
use std::collections::HashSet;

use crate::client::{PostInsightsResponse, ReplyItem, ThreadsClient};
use crate::config::AppConfig;
use crate::error::{CliError, ErrorCategory};
use crate::output::{print_success, OutputMode};
use crate::store::{retry_with_backoff, InsightRow, ReplyRow, RetryOperation, RetryPolicy, Store};

#[derive(Debug, Subcommand)]
pub enum PostSubcommand {
    Get(GetArgs),
    Insights(InsightsArgs),
    Replies(RepliesArgs),
}

#[derive(Debug, Args)]
pub struct GetArgs {
    #[arg(long)]
    pub id: String,
}

#[derive(Debug, Args)]
pub struct InsightsArgs {
    #[arg(long)]
    pub id: String,
    #[arg(long, default_value_t = false)]
    pub refresh: bool,
}

#[derive(Debug, Args)]
pub struct RepliesArgs {
    #[arg(long)]
    pub id: String,
    #[arg(long, default_value_t = false)]
    pub refresh: bool,
    #[arg(long, default_value_t = 20)]
    pub limit: usize,
}

#[derive(Debug, Serialize)]
struct PostGetData {
    id: String,
    text: Option<String>,
    permalink: Option<String>,
    timestamp: Option<String>,
    username: Option<String>,
    shortcode: Option<String>,
    accessible: bool,
}

#[derive(Debug, Serialize)]
struct PostInsightsData {
    post_id: String,
    source: String,
    snapshot: Option<InsightData>,
}

#[derive(Debug, Serialize)]
struct InsightData {
    fetched_at: String,
    views: Option<i64>,
    likes: Option<i64>,
    replies: Option<i64>,
    reposts: Option<i64>,
    quotes: Option<i64>,
    shares: Option<i64>,
}

#[derive(Debug, Serialize)]
struct PostRepliesData {
    post_id: String,
    source: String,
    fetched_count: usize,
    next_cursor: Option<String>,
    replies: Vec<ReplyData>,
}

#[derive(Debug, Serialize)]
struct ReplyData {
    reply_id: String,
    author_username: Option<String>,
    text: Option<String>,
    posted_at: Option<String>,
    created_at: String,
}

pub async fn run(
    command: super::PostCommand,
    app: &AppConfig,
    store: &Store,
    output_mode: OutputMode,
) -> Result<(), CliError> {
    let client = ThreadsClient::from_config(app)?;
    match command.command {
        PostSubcommand::Get(args) => {
            if args.id.trim().is_empty() {
                return Err(CliError::new(
                    ErrorCategory::Validation,
                    "post id must be non-empty",
                ));
            }
            let access_token = require_access_token(store)?;
            let result = retry_with_backoff(
                RetryPolicy {
                    max_retries: 3,
                    base_delay_ms: 100,
                },
                RetryOperation::SafeRead,
                || async { client.fetch_post_details(&access_token, &args.id).await },
            )
            .await;
            match result {
                Ok(details) => {
                    let data = PostGetData {
                        id: details.id,
                        text: details.text,
                        permalink: details.permalink,
                        timestamp: details.timestamp,
                        username: details.username,
                        shortcode: details.shortcode,
                        accessible: true,
                    };
                    print_success(output_mode, "post get", format_post_get_human(&data), data);
                }
                Err(ref err) if matches!(err.category, ErrorCategory::Auth | ErrorCategory::Api) => {
                    let data = PostGetData {
                        id: args.id.clone(),
                        text: None,
                        permalink: None,
                        timestamp: None,
                        username: None,
                        shortcode: None,
                        accessible: false,
                    };
                    print_success(
                        output_mode,
                        "post get",
                        format!("Post {} is not accessible: {}", args.id, err.message),
                        data,
                    );
                }
                Err(err) => return Err(err),
            }
            Ok(())
        }
        PostSubcommand::Insights(args) => {
            if args.id.trim().is_empty() {
                return Err(CliError::new(
                    ErrorCategory::Validation,
                    "post id must be non-empty",
                ));
            }
            let (source, snapshot) = if args.refresh {
                let access_token = require_access_token(store)?;
                let response = retry_with_backoff(
                    RetryPolicy {
                        max_retries: 3,
                        base_delay_ms: 100,
                    },
                    RetryOperation::SafeRead,
                    || async { client.fetch_post_insights(&access_token, &args.id).await },
                )
                .await?;
                let normalized = normalize_insights(&response);
                store.insert_insight_snapshot(
                    &args.id,
                    normalized.views,
                    normalized.likes,
                    normalized.replies,
                    normalized.reposts,
                    normalized.quotes,
                    normalized.shares,
                    &serde_json::to_string(&response).unwrap_or_else(|_| "{}".to_string()),
                )?;
                ("live".to_string(), Some(normalized))
            } else {
                match store.latest_insight(&args.id)? {
                    Some(cached) => ("cache".to_string(), Some(to_insight(cached))),
                    None => {
                        let access_token = require_access_token(store)?;
                        let response = retry_with_backoff(
                            RetryPolicy {
                                max_retries: 3,
                                base_delay_ms: 100,
                            },
                            RetryOperation::SafeRead,
                            || async { client.fetch_post_insights(&access_token, &args.id).await },
                        )
                        .await?;
                        let normalized = normalize_insights(&response);
                        store.insert_insight_snapshot(
                            &args.id,
                            normalized.views,
                            normalized.likes,
                            normalized.replies,
                            normalized.reposts,
                            normalized.quotes,
                            normalized.shares,
                            &serde_json::to_string(&response).unwrap_or_else(|_| "{}".to_string()),
                        )?;
                        ("live".to_string(), Some(normalized))
                    }
                }
            };
            let data = PostInsightsData {
                post_id: args.id,
                source,
                snapshot,
            };
            print_success(
                output_mode,
                "post insights",
                format_post_insights_human(&data),
                data,
            );
            Ok(())
        }
        PostSubcommand::Replies(args) => {
            if args.id.trim().is_empty() {
                return Err(CliError::new(
                    ErrorCategory::Validation,
                    "post id must be non-empty",
                ));
            }
            if args.limit == 0 {
                return Err(CliError::new(
                    ErrorCategory::Validation,
                    "limit must be >= 1",
                ));
            }
            let (source, fetched_count, next_cursor) = if args.refresh {
                let access_token = require_access_token(store)?;
                let page = retry_with_backoff(
                    RetryPolicy {
                        max_retries: 3,
                        base_delay_ms: 100,
                    },
                    RetryOperation::SafeRead,
                    || async {
                        client
                            .fetch_replies(&access_token, &args.id, Some(args.limit as u32), None)
                            .await
                    },
                )
                .await?;
                persist_replies(store, &args.id, &page.data)?;
                let next_cursor = extract_next_cursor(page.paging.as_ref());
                store.upsert_reply_fetch_state(
                    &args.id,
                    next_cursor.as_deref(),
                    Some(&serde_json::to_string(&page).unwrap_or_else(|_| "{}".to_string())),
                )?;
                ("live".to_string(), page.data.len(), next_cursor)
            } else {
                let state = store.reply_fetch_state(&args.id)?;
                let cached = store.latest_replies(&args.id, args.limit)?;
                if cached.is_empty() {
                    let access_token = require_access_token(store)?;
                    let page = retry_with_backoff(
                        RetryPolicy {
                            max_retries: 3,
                            base_delay_ms: 100,
                        },
                        RetryOperation::SafeRead,
                        || async {
                            client
                                .fetch_replies(&access_token, &args.id, Some(args.limit as u32), None)
                                .await
                        },
                    )
                    .await?;
                    persist_replies(store, &args.id, &page.data)?;
                    let next_cursor = extract_next_cursor(page.paging.as_ref());
                    store.upsert_reply_fetch_state(
                        &args.id,
                        next_cursor.as_deref(),
                        Some(&serde_json::to_string(&page).unwrap_or_else(|_| "{}".to_string())),
                    )?;
                    ("live".to_string(), page.data.len(), next_cursor)
                } else {
                    let current_next = state.and_then(|s| s.next_cursor);
                    if let Some(after) = current_next.as_deref() {
                        let access_token = require_access_token(store)?;
                        let page = retry_with_backoff(
                            RetryPolicy {
                                max_retries: 3,
                                base_delay_ms: 100,
                            },
                            RetryOperation::SafeRead,
                            || async {
                                client
                                    .fetch_replies(
                                        &access_token,
                                        &args.id,
                                        Some(args.limit as u32),
                                        Some(after),
                                    )
                                    .await
                            },
                        )
                        .await?;
                        persist_replies(store, &args.id, &page.data)?;
                        let next_cursor = extract_next_cursor(page.paging.as_ref());
                        store.upsert_reply_fetch_state(
                            &args.id,
                            next_cursor.as_deref(),
                            Some(&serde_json::to_string(&page).unwrap_or_else(|_| "{}".to_string())),
                        )?;
                        ("live".to_string(), page.data.len(), next_cursor)
                    } else {
                        ("cache".to_string(), 0, None)
                    }
                }
            };
            let mut seen_reply_ids = HashSet::new();
            let mut replies: Vec<ReplyData> = store
                .list_posts_by_parent(&args.id, args.limit)?
                .into_iter()
                .filter(|row| row.kind == "reply")
                .map(to_reply_from_post)
                .inspect(|reply| {
                    seen_reply_ids.insert(reply.reply_id.clone());
                })
                .collect();
            for reply in store.latest_replies(&args.id, args.limit)? {
                if !seen_reply_ids.contains(&reply.threads_reply_id) {
                    replies.push(to_reply(reply));
                }
            }
            let data = PostRepliesData {
                post_id: args.id,
                source,
                fetched_count,
                next_cursor,
                replies,
            };
            print_success(
                output_mode,
                "post replies",
                format_post_replies_human(&data),
                data,
            );
            Ok(())
        }
    }
}

fn to_insight(row: InsightRow) -> InsightData {
    InsightData {
        fetched_at: row.fetched_at,
        views: row.views,
        likes: row.likes,
        replies: row.replies,
        reposts: row.reposts,
        quotes: row.quotes,
        shares: row.shares,
    }
}

fn to_reply(row: ReplyRow) -> ReplyData {
    ReplyData {
        reply_id: row.threads_reply_id,
        author_username: row.author_username,
        text: row.text,
        posted_at: row.posted_at,
        created_at: row.created_at,
    }
}

fn to_reply_from_post(row: crate::store::PostRow) -> ReplyData {
    ReplyData {
        reply_id: row.threads_post_id,
        author_username: None,
        text: Some(row.text),
        posted_at: row.published_at,
        created_at: row.created_at,
    }
}

fn require_access_token(store: &Store) -> Result<String, CliError> {
    store.latest_access_token()?.ok_or_else(|| {
        CliError::new(
            ErrorCategory::Auth,
            "no access token found; run auth login first",
        )
    })
}

fn normalize_insights(response: &PostInsightsResponse) -> InsightData {
    let mut data = InsightData {
        fetched_at: chrono::Utc::now().to_rfc3339(),
        views: None,
        likes: None,
        replies: None,
        reposts: None,
        quotes: None,
        shares: None,
    };
    for metric in &response.data {
        let value = metric.values.iter().find_map(|v| v.value);
        match metric.name.as_str() {
            "views" => data.views = value,
            "likes" => data.likes = value,
            "replies" => data.replies = value,
            "reposts" => data.reposts = value,
            "quotes" => data.quotes = value,
            "shares" => data.shares = value,
            _ => {}
        }
    }
    data
}

fn persist_replies(store: &Store, post_id: &str, replies: &[ReplyItem]) -> Result<(), CliError> {
    for reply in replies {
        store.upsert_reply(
            &reply.id,
            post_id,
            reply.username.as_deref(),
            reply.text.as_deref(),
            reply.timestamp.as_deref(),
            &serde_json::to_string(reply).unwrap_or_else(|_| "{}".to_string()),
        )?;
    }
    Ok(())
}

fn extract_next_cursor(state: Option<&crate::client::Paging>) -> Option<String> {
    state
        .and_then(|paging| paging.cursors.as_ref())
        .and_then(|cursors| cursors.after.clone())
}

fn format_post_get_human(data: &PostGetData) -> String {
    format!(
        "Post {} fetched.{}",
        data.id,
        data.text
            .as_ref()
            .map(|t| format!(" {}", truncate(t, 80)))
            .unwrap_or_default()
    )
}

fn format_post_insights_human(data: &PostInsightsData) -> String {
    match &data.snapshot {
        Some(snapshot) => format!(
            "Insights {} from {}: views={:?} likes={:?} replies={:?}",
            data.post_id, data.source, snapshot.views, snapshot.likes, snapshot.replies
        ),
        None => format!("Insights {} from {}: no data", data.post_id, data.source),
    }
}

fn format_post_replies_human(data: &PostRepliesData) -> String {
    format!(
        "Replies {} from {}: fetched={} showing={} next_cursor={}",
        data.post_id,
        data.source,
        data.fetched_count,
        data.replies.len(),
        data.next_cursor.as_deref().unwrap_or("none")
    )
}

fn truncate(value: &str, max: usize) -> String {
    if value.chars().count() <= max {
        value.to_string()
    } else {
        let mut out = value.chars().take(max).collect::<String>();
        out.push('…');
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    use crate::client::{InsightMetric, InsightValue, PostInsightsResponse};
    use crate::config::{AppConfig, DefaultsConfig, OAuthConfig, StorageConfig, ThreadsConfig};
    use crate::store::Store;

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

    fn add_token(store: &Store) {
        let account_id = store
            .upsert_account("user-id", Some("tester"), Some("Tester"))
            .expect("upsert account");
        store
            .insert_token(
                account_id,
                "test-token",
                Some("refresh-token"),
                Some("2024-01-01T00:00:00Z"),
                None,
                "{}",
            )
            .expect("insert token");
    }

    fn spawn_mock_server(
        expected_requests: usize,
        responder: impl Fn(&str) -> (String, String) + Send + Sync + 'static,
    ) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind server");
        let addr = listener.local_addr().expect("local addr");
        let responder = std::sync::Arc::new(responder);
        thread::spawn(move || {
            for _ in 0..expected_requests {
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
                let (status, body) = responder(path);
                let response = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                stream.write_all(response.as_bytes()).expect("write response");
                stream.flush().expect("flush");
            }
        });
        format!("http://{}", addr)
    }

    #[test]
    fn normalize_insights_maps_expected_metrics() {
        let response = PostInsightsResponse {
            data: vec![
                InsightMetric {
                    name: "views".to_string(),
                    title: None,
                    description: None,
                    values: vec![InsightValue {
                        value: Some(42),
                        end_time: None,
                    }],
                },
                InsightMetric {
                    name: "likes".to_string(),
                    title: None,
                    description: None,
                    values: vec![InsightValue {
                        value: Some(5),
                        end_time: None,
                    }],
                },
            ],
        };
        let normalized = normalize_insights(&response);
        assert_eq!(normalized.views, Some(42));
        assert_eq!(normalized.likes, Some(5));
        assert_eq!(normalized.replies, None);
    }

    #[test]
    fn extract_next_cursor_prefers_cursors_after() {
        let paging = crate::client::Paging {
            cursors: Some(crate::client::Cursors {
                before: Some("b".to_string()),
                after: Some("a".to_string()),
            }),
            next: Some("https://example.test".to_string()),
        };
        assert_eq!(extract_next_cursor(Some(&paging)), Some("a".to_string()));
        assert_eq!(extract_next_cursor(None), None);
    }

    #[test]
    fn truncate_short_and_long_text() {
        assert_eq!(truncate("abc", 5), "abc");
        assert_eq!(truncate("abcdef", 3), "abc…");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn insights_uses_cache_without_refresh() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("threads.db");
        let db_path_str = db_path.to_str().expect("db path");
        let app = test_config(db_path_str, "http://127.0.0.1:1");
        write_secret_file(&app);
        let store = Store::open(db_path_str).expect("open store");
        store.run_migrations().expect("run migrations");
        add_token(&store);
        store
            .insert_insight_snapshot("post_1", Some(50), Some(7), Some(2), Some(0), Some(0), Some(0), "{}")
            .expect("insert cached insight");

        run(
            super::super::PostCommand {
                command: PostSubcommand::Insights(InsightsArgs {
                    id: "post_1".to_string(),
                    refresh: false,
                }),
            },
            &app,
            &store,
            OutputMode::Json,
        )
        .await
        .expect("insights should use cache");

        let cached = store
            .latest_insight("post_1")
            .expect("latest insight")
            .expect("insight row");
        assert_eq!(cached.views, Some(50));
        assert_eq!(cached.likes, Some(7));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn insights_refresh_fetches_live_and_persists_snapshot() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("threads.db");
        let db_path_str = db_path.to_str().expect("db path");
        let base_url = spawn_mock_server(1, |path| {
            if path.contains("/post_9/insights?") {
                (
                    "200 OK".to_string(),
                    serde_json::json!({
                        "data":[
                            {"name":"views","values":[{"value":101}]},
                            {"name":"likes","values":[{"value":11}]},
                            {"name":"replies","values":[{"value":4}]}
                        ]
                    })
                    .to_string(),
                )
            } else {
                (
                    "404 Not Found".to_string(),
                    serde_json::json!({"error":{"message":"not found"}}).to_string(),
                )
            }
        });
        let app = test_config(db_path_str, &base_url);
        write_secret_file(&app);
        let store = Store::open(db_path_str).expect("open store");
        store.run_migrations().expect("run migrations");
        add_token(&store);

        run(
            super::super::PostCommand {
                command: PostSubcommand::Insights(InsightsArgs {
                    id: "post_9".to_string(),
                    refresh: true,
                }),
            },
            &app,
            &store,
            OutputMode::Json,
        )
        .await
        .expect("insights refresh should succeed");

        let insight = store
            .latest_insight("post_9")
            .expect("latest insight")
            .expect("insight row");
        assert_eq!(insight.views, Some(101));
        assert_eq!(insight.likes, Some(11));
        assert_eq!(insight.replies, Some(4));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn replies_refresh_respects_limit_and_updates_cursor() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("threads.db");
        let db_path_str = db_path.to_str().expect("db path");
        let base_url = spawn_mock_server(1, |path| {
            if path.contains("/post_1/replies?") && path.contains("limit=1") {
                (
                    "200 OK".to_string(),
                    serde_json::json!({
                        "data":[
                            {"id":"reply_1","text":"first","username":"alice","timestamp":"2024-01-01T00:00:00Z"}
                        ],
                        "paging":{"cursors":{"after":"cursor_next"}}
                    })
                    .to_string(),
                )
            } else {
                (
                    "404 Not Found".to_string(),
                    serde_json::json!({"error":{"message":"not found"}}).to_string(),
                )
            }
        });
        let app = test_config(db_path_str, &base_url);
        write_secret_file(&app);
        let store = Store::open(db_path_str).expect("open store");
        store.run_migrations().expect("run migrations");
        add_token(&store);

        run(
            super::super::PostCommand {
                command: PostSubcommand::Replies(RepliesArgs {
                    id: "post_1".to_string(),
                    refresh: true,
                    limit: 1,
                }),
            },
            &app,
            &store,
            OutputMode::Json,
        )
        .await
        .expect("replies refresh should succeed");

        let replies = store.latest_replies("post_1", 10).expect("latest replies");
        assert_eq!(replies.len(), 1);
        assert_eq!(replies[0].threads_reply_id, "reply_1");
        let state = store
            .reply_fetch_state("post_1")
            .expect("reply fetch state")
            .expect("state row");
        assert_eq!(state.next_cursor.as_deref(), Some("cursor_next"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn replies_without_refresh_uses_cache_when_no_next_cursor() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("threads.db");
        let db_path_str = db_path.to_str().expect("db path");
        let app = test_config(db_path_str, "http://127.0.0.1:1");
        write_secret_file(&app);
        let store = Store::open(db_path_str).expect("open store");
        store.run_migrations().expect("run migrations");
        add_token(&store);
        store
            .upsert_reply(
                "reply_cached",
                "post_cached",
                Some("bob"),
                Some("cached text"),
                Some("2024-01-01T00:00:00Z"),
                "{}",
            )
            .expect("upsert cached reply");
        store
            .upsert_reply_fetch_state("post_cached", None, Some("{}"))
            .expect("set cached state");

        run(
            super::super::PostCommand {
                command: PostSubcommand::Replies(RepliesArgs {
                    id: "post_cached".to_string(),
                    refresh: false,
                    limit: 5,
                }),
            },
            &app,
            &store,
            OutputMode::Json,
        )
        .await
        .expect("replies should come from cache");

        let replies = store.latest_replies("post_cached", 10).expect("latest replies");
        assert_eq!(replies.len(), 1);
        assert_eq!(replies[0].threads_reply_id, "reply_cached");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn replies_rejects_zero_limit() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("threads.db");
        let db_path_str = db_path.to_str().expect("db path");
        let app = test_config(db_path_str, "http://127.0.0.1:1");
        write_secret_file(&app);
        let store = Store::open(db_path_str).expect("open store");
        store.run_migrations().expect("run migrations");
        add_token(&store);

        let err = run(
            super::super::PostCommand {
                command: PostSubcommand::Replies(RepliesArgs {
                    id: "post_1".to_string(),
                    refresh: false,
                    limit: 0,
                }),
            },
            &app,
            &store,
            OutputMode::Json,
        )
        .await
        .expect_err("limit=0 must fail");
        assert_eq!(err.category.as_code(), "VALIDATION_ERROR");
        assert_eq!(err.message, "limit must be >= 1");
    }
}
