use clap::{Args, Subcommand};
use serde::Serialize;

use crate::client::ThreadsClient;
use crate::config::AppConfig;
use crate::error::{CliError, ErrorCategory};
use crate::output::{print_success, OutputMode};
use crate::store::{retry_with_backoff, RetryOperation, RetryPolicy, Store};

#[derive(Debug, Subcommand)]
pub enum SearchSubcommand {
    Posts(SearchPostsArgs),
}

#[derive(Debug, Args)]
pub struct SearchPostsArgs {
    #[arg(long)]
    pub query: String,
    #[arg(long, default_value = "top")]
    pub r#type: String,
    #[arg(long, default_value_t = 10)]
    pub limit: usize,
    #[arg(long, default_value_t = false)]
    pub refresh: bool,
}

#[derive(Debug, Serialize)]
struct SearchPostsData {
    query: String,
    search_type: String,
    source: String,
    results: Vec<SearchPostItem>,
}

#[derive(Debug, Serialize)]
struct SearchPostItem {
    id: String,
    username: Option<String>,
    text: Option<String>,
    timestamp: Option<String>,
    permalink: Option<String>,
    like_count: Option<i64>,
    reply_count: Option<i64>,
}

fn require_access_token(store: &Store) -> Result<String, CliError> {
    store.latest_access_token()?.ok_or_else(|| {
        CliError::new(ErrorCategory::Auth, "not authenticated; run auth login first")
    })
}

pub async fn run(
    command: super::SearchCommand,
    app: &AppConfig,
    store: &Store,
    output_mode: OutputMode,
) -> Result<(), CliError> {
    match command.command {
        SearchSubcommand::Posts(args) => {
            if args.query.trim().is_empty() {
                return Err(CliError::new(
                    ErrorCategory::Validation,
                    "search query must be non-empty",
                ));
            }
            let search_type = args.r#type.to_uppercase();
            if search_type != "TOP" && search_type != "RECENT" {
                return Err(CliError::new(
                    ErrorCategory::Validation,
                    "search type must be top or recent",
                ));
            }
            if args.limit == 0 {
                return Err(CliError::new(
                    ErrorCategory::Validation,
                    "limit must be >= 1",
                ));
            }

            let (source, results) = if args.refresh {
                fetch_and_cache_search(app, store, &args.query, &search_type, args.limit).await?
            } else {
                let cached = store.cached_search_results(&args.query, &search_type, args.limit)?;
                if cached.is_empty() {
                    fetch_and_cache_search(app, store, &args.query, &search_type, args.limit).await?
                } else {
                    let items = cached
                        .into_iter()
                        .map(|r| SearchPostItem {
                            id: r.threads_post_id,
                            username: r.username,
                            text: r.text,
                            timestamp: r.timestamp,
                            permalink: r.permalink,
                            like_count: r.like_count,
                            reply_count: r.reply_count,
                        })
                        .collect();
                    ("cache".to_string(), items)
                }
            };

            let data = SearchPostsData {
                query: args.query.clone(),
                search_type: search_type.clone(),
                source,
                results,
            };
            print_success(
                output_mode,
                "search posts",
                format_search_human(&data),
                data,
            );
            Ok(())
        }
    }
}

async fn fetch_and_cache_search(
    app: &AppConfig,
    store: &Store,
    query: &str,
    search_type: &str,
    limit: usize,
) -> Result<(String, Vec<SearchPostItem>), CliError> {
    let access_token = require_access_token(store)?;
    let client = ThreadsClient::from_config(app)?;
    let response = retry_with_backoff(
        RetryPolicy {
            max_retries: 3,
            base_delay_ms: 100,
        },
        RetryOperation::SafeRead,
        || async { client.keyword_search(&access_token, query, search_type).await },
    )
    .await?;

    let cache_rows: Vec<_> = response
        .data
        .iter()
        .map(|item| {
            (
                item.id.clone(),
                item.username.clone(),
                item.text.clone(),
                item.permalink.clone(),
                item.timestamp.clone(),
                item.like_count,
                item.reply_count,
                serde_json::to_string(item).unwrap_or_else(|_| "{}".to_string()),
            )
        })
        .collect();
    store.insert_search_results(query, search_type, &cache_rows)?;

    let items: Vec<SearchPostItem> = response
        .data
        .into_iter()
        .take(limit)
        .map(|item| SearchPostItem {
            id: item.id,
            username: item.username,
            text: item.text,
            timestamp: item.timestamp,
            permalink: item.permalink,
            like_count: item.like_count,
            reply_count: item.reply_count,
        })
        .collect();
    Ok(("live".to_string(), items))
}

fn format_search_human(data: &SearchPostsData) -> String {
    if data.results.is_empty() {
        return format!(
            "Search '{}' ({}): no results (source={})",
            data.query, data.search_type, data.source
        );
    }
    let mut lines = vec![format!(
        "Search '{}' ({}) from {}: {} results",
        data.query,
        data.search_type,
        data.source,
        data.results.len()
    )];
    for item in &data.results {
        let username = item.username.as_deref().unwrap_or("unknown");
        let text_preview = item
            .text
            .as_deref()
            .unwrap_or("")
            .chars()
            .take(80)
            .collect::<String>();
        let likes = item.like_count.unwrap_or(0);
        let replies = item.reply_count.unwrap_or(0);
        lines.push(format!(
            "  [{}] @{}: {} (♥{} 💬{})",
            item.id, username, text_preview, likes, replies
        ));
    }
    lines.join("\n")
}
