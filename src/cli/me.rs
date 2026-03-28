use clap::{Args, Subcommand};
use serde::Serialize;

use crate::client::ThreadsClient;
use crate::config::AppConfig;
use crate::error::{CliError, ErrorCategory};
use crate::output::{print_success, OutputMode};
use crate::store::{retry_with_backoff, RetryOperation, RetryPolicy, Store};

#[derive(Debug, Subcommand)]
pub enum MeSubcommand {
    Threads(MeThreadsArgs),
    Replies(MeRepliesArgs),
}

#[derive(Debug, Args)]
pub struct MeThreadsArgs {
    #[arg(long, default_value_t = 10)]
    pub limit: usize,
    #[arg(long, default_value_t = false)]
    pub refresh: bool,
}

#[derive(Debug, Args)]
pub struct MeRepliesArgs {
    #[arg(long, default_value_t = 10)]
    pub limit: usize,
    #[arg(long, default_value_t = false)]
    pub refresh: bool,
}

#[derive(Debug, Serialize)]
struct MeThreadsData {
    source: String,
    threads: Vec<ThreadItem>,
}

#[derive(Debug, Serialize)]
struct ThreadItem {
    id: String,
    text: Option<String>,
    permalink: Option<String>,
    timestamp: Option<String>,
    username: Option<String>,
}

#[derive(Debug, Serialize)]
struct MeRepliesData {
    source: String,
    replies: Vec<ReplyItem>,
}

#[derive(Debug, Serialize)]
struct ReplyItem {
    id: String,
    reply_to_id: Option<String>,
    text: Option<String>,
    permalink: Option<String>,
    timestamp: Option<String>,
    username: Option<String>,
}

fn require_access_token(store: &Store) -> Result<String, CliError> {
    store.latest_access_token()?.ok_or_else(|| {
        CliError::new(ErrorCategory::Auth, "not authenticated; run auth login first")
    })
}

pub async fn run(
    command: super::MeCommand,
    app: &AppConfig,
    store: &Store,
    output_mode: OutputMode,
) -> Result<(), CliError> {
    match command.command {
        MeSubcommand::Threads(args) => run_threads(args, app, store, output_mode).await,
        MeSubcommand::Replies(args) => run_replies(args, app, store, output_mode).await,
    }
}

async fn run_threads(
    args: MeThreadsArgs,
    app: &AppConfig,
    store: &Store,
    output_mode: OutputMode,
) -> Result<(), CliError> {
    if args.limit == 0 {
        return Err(CliError::new(
            ErrorCategory::Validation,
            "limit must be >= 1",
        ));
    }

    let (source, threads) = if args.refresh {
        fetch_and_cache_threads(app, store, args.limit).await?
    } else {
        let cached = store.list_own_threads(args.limit)?;
        if cached.is_empty() {
            fetch_and_cache_threads(app, store, args.limit).await?
        } else {
            let items = cached
                .into_iter()
                .map(|r| ThreadItem {
                    id: r.threads_post_id,
                    text: r.text,
                    permalink: r.permalink,
                    timestamp: r.timestamp,
                    username: r.username,
                })
                .collect();
            ("cache".to_string(), items)
        }
    };

    let data = MeThreadsData { source, threads };
    print_success(
        output_mode,
        "me threads",
        format_threads_human(&data),
        data,
    );
    Ok(())
}

async fn fetch_and_cache_threads(
    app: &AppConfig,
    store: &Store,
    limit: usize,
) -> Result<(String, Vec<ThreadItem>), CliError> {
    let access_token = require_access_token(store)?;
    let client = ThreadsClient::from_config(app)?;
    let response = retry_with_backoff(
        RetryPolicy {
            max_retries: 3,
            base_delay_ms: 100,
        },
        RetryOperation::SafeRead,
        || async {
            client
                .fetch_own_threads(&access_token, Some(limit as u32), None)
                .await
        },
    )
    .await?;

    for item in &response.data {
        store.upsert_own_thread(
            &item.id,
            item.text.as_deref(),
            item.permalink.as_deref(),
            item.timestamp.as_deref(),
            item.username.as_deref(),
            &serde_json::to_string(item).unwrap_or_else(|_| "{}".to_string()),
        )?;
    }

    let items: Vec<ThreadItem> = response
        .data
        .into_iter()
        .take(limit)
        .map(|item| ThreadItem {
            id: item.id,
            text: item.text,
            permalink: item.permalink,
            timestamp: item.timestamp,
            username: item.username,
        })
        .collect();
    Ok(("live".to_string(), items))
}

async fn run_replies(
    args: MeRepliesArgs,
    app: &AppConfig,
    store: &Store,
    output_mode: OutputMode,
) -> Result<(), CliError> {
    if args.limit == 0 {
        return Err(CliError::new(
            ErrorCategory::Validation,
            "limit must be >= 1",
        ));
    }

    let (source, replies) = if args.refresh {
        fetch_and_cache_replies(app, store, args.limit).await?
    } else {
        let cached = store.list_own_replies(args.limit)?;
        if cached.is_empty() {
            fetch_and_cache_replies(app, store, args.limit).await?
        } else {
            let items = cached
                .into_iter()
                .map(|r| ReplyItem {
                    id: r.threads_post_id,
                    reply_to_id: r.reply_to_id,
                    text: r.text,
                    permalink: r.permalink,
                    timestamp: r.timestamp,
                    username: r.username,
                })
                .collect();
            ("cache".to_string(), items)
        }
    };

    let data = MeRepliesData { source, replies };
    print_success(
        output_mode,
        "me replies",
        format_replies_human(&data),
        data,
    );
    Ok(())
}

async fn fetch_and_cache_replies(
    app: &AppConfig,
    store: &Store,
    limit: usize,
) -> Result<(String, Vec<ReplyItem>), CliError> {
    let access_token = require_access_token(store)?;
    let client = ThreadsClient::from_config(app)?;
    let response = retry_with_backoff(
        RetryPolicy {
            max_retries: 3,
            base_delay_ms: 100,
        },
        RetryOperation::SafeRead,
        || async {
            client
                .fetch_own_replies(&access_token, Some(limit as u32), None)
                .await
        },
    )
    .await?;

    for item in &response.data {
        store.upsert_own_reply(
            &item.id,
            item.reply_to_id.as_deref(),
            item.text.as_deref(),
            item.permalink.as_deref(),
            item.timestamp.as_deref(),
            item.username.as_deref(),
            &serde_json::to_string(item).unwrap_or_else(|_| "{}".to_string()),
        )?;
    }

    let items: Vec<ReplyItem> = response
        .data
        .into_iter()
        .take(limit)
        .map(|item| ReplyItem {
            id: item.id,
            reply_to_id: item.reply_to_id,
            text: item.text,
            permalink: item.permalink,
            timestamp: item.timestamp,
            username: item.username,
        })
        .collect();
    Ok(("live".to_string(), items))
}

fn format_threads_human(data: &MeThreadsData) -> String {
    if data.threads.is_empty() {
        return format!("No threads found (source={})", data.source);
    }
    let mut lines = vec![format!(
        "Your threads from {}: {} results",
        data.source,
        data.threads.len()
    )];
    for item in &data.threads {
        let text_preview = item
            .text
            .as_deref()
            .unwrap_or("")
            .chars()
            .take(80)
            .collect::<String>();
        lines.push(format!(
            "  [{}] {} ({})",
            item.id,
            text_preview,
            item.permalink.as_deref().unwrap_or("no link")
        ));
    }
    lines.join("\n")
}

fn format_replies_human(data: &MeRepliesData) -> String {
    if data.replies.is_empty() {
        return format!("No replies found (source={})", data.source);
    }
    let mut lines = vec![format!(
        "Your replies from {}: {} results",
        data.source,
        data.replies.len()
    )];
    for item in &data.replies {
        let text_preview = item
            .text
            .as_deref()
            .unwrap_or("")
            .chars()
            .take(80)
            .collect::<String>();
        let parent = item.reply_to_id.as_deref().unwrap_or("unknown");
        lines.push(format!(
            "  [{}] reply-to={} {} ({})",
            item.id,
            parent,
            text_preview,
            item.permalink.as_deref().unwrap_or("no link")
        ));
    }
    lines.join("\n")
}
