use clap::{Args, Subcommand};
use serde::Serialize;

use crate::client::ThreadsClient;
use crate::config::AppConfig;
use crate::error::{CliError, ErrorCategory};
use crate::output::{print_success, OutputMode};
use crate::store::{retry_with_backoff, RetryOperation, RetryPolicy, Store};

#[derive(Debug, Subcommand)]
pub enum ActivitySubcommand {
    Replies(ActivityRepliesArgs),
}

#[derive(Debug, Args)]
pub struct ActivityRepliesArgs {
    #[arg(long, default_value_t = 10)]
    pub recent: usize,
    #[arg(long, default_value_t = false)]
    pub refresh: bool,
}

#[derive(Debug, Serialize)]
struct ActivityRepliesData {
    source: String,
    posts_checked: usize,
    total_replies: usize,
    posts: Vec<PostWithReplies>,
}

#[derive(Debug, Serialize)]
struct PostWithReplies {
    post_id: String,
    post_text: Option<String>,
    replies: Vec<ActivityReplyItem>,
}

#[derive(Debug, Serialize)]
struct ActivityReplyItem {
    reply_id: String,
    username: Option<String>,
    text: Option<String>,
    timestamp: Option<String>,
}

fn require_access_token(store: &Store) -> Result<String, CliError> {
    store.latest_access_token()?.ok_or_else(|| {
        CliError::new(ErrorCategory::Auth, "not authenticated; run auth login first")
    })
}

pub async fn run(
    command: super::ActivityCommand,
    app: &AppConfig,
    store: &Store,
    output_mode: OutputMode,
) -> Result<(), CliError> {
    match command.command {
        ActivitySubcommand::Replies(args) => {
            if args.recent == 0 {
                return Err(CliError::new(
                    ErrorCategory::Validation,
                    "recent must be >= 1",
                ));
            }

            let access_token = require_access_token(store)?;
            let client = ThreadsClient::from_config(app)?;

            // Fetch own threads to know which posts to check
            let (source, own_threads) = if args.refresh {
                let response = retry_with_backoff(
                    RetryPolicy {
                        max_retries: 3,
                        base_delay_ms: 100,
                    },
                    RetryOperation::SafeRead,
                    || async {
                        client
                            .fetch_own_threads(&access_token, Some(args.recent as u32), None)
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
                ("live".to_string(), response.data)
            } else {
                let cached = store.list_own_threads(args.recent)?;
                if cached.is_empty() {
                    let response = retry_with_backoff(
                        RetryPolicy {
                            max_retries: 3,
                            base_delay_ms: 100,
                        },
                        RetryOperation::SafeRead,
                        || async {
                            client
                                .fetch_own_threads(&access_token, Some(args.recent as u32), None)
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
                    ("live".to_string(), response.data)
                } else {
                    let items = cached
                        .into_iter()
                        .map(|r| crate::client::UserThreadItem {
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

            // Fetch account identity to filter out own replies
            let identity = client.fetch_account_identity(&access_token).await?;
            let own_username = identity.username.unwrap_or_default();

            let mut posts_with_replies = Vec::new();
            let mut total_replies = 0;

            for thread in &own_threads {
                let replies_result = retry_with_backoff(
                    RetryPolicy {
                        max_retries: 3,
                        base_delay_ms: 100,
                    },
                    RetryOperation::SafeRead,
                    || async {
                        client
                            .fetch_replies(&access_token, &thread.id, Some(25), None)
                            .await
                    },
                )
                .await;

                match replies_result {
                    Ok(page) => {
                        // Cache replies
                        for reply in &page.data {
                            store.upsert_reply(
                                &reply.id,
                                &thread.id,
                                reply.username.as_deref(),
                                reply.text.as_deref(),
                                reply.timestamp.as_deref(),
                                &serde_json::to_string(reply)
                                    .unwrap_or_else(|_| "{}".to_string()),
                            )?;
                        }

                        // Filter out own replies
                        let other_replies: Vec<ActivityReplyItem> = page
                            .data
                            .into_iter()
                            .filter(|r| {
                                r.username
                                    .as_deref()
                                    .map(|u| u != own_username)
                                    .unwrap_or(true)
                            })
                            .map(|r| ActivityReplyItem {
                                reply_id: r.id,
                                username: r.username,
                                text: r.text,
                                timestamp: r.timestamp,
                            })
                            .collect();

                        if !other_replies.is_empty() {
                            total_replies += other_replies.len();
                            posts_with_replies.push(PostWithReplies {
                                post_id: thread.id.clone(),
                                post_text: thread.text.clone(),
                                replies: other_replies,
                            });
                        }
                    }
                    Err(_) => {
                        // Partial failure: skip this post, continue with others
                        continue;
                    }
                }
            }

            let data = ActivityRepliesData {
                source,
                posts_checked: own_threads.len(),
                total_replies,
                posts: posts_with_replies,
            };
            print_success(
                output_mode,
                "activity replies",
                format_activity_human(&data),
                data,
            );
            Ok(())
        }
    }
}

fn format_activity_human(data: &ActivityRepliesData) -> String {
    if data.posts.is_empty() {
        return format!(
            "No replies from others on your {} most recent posts (source={})",
            data.posts_checked, data.source
        );
    }
    let mut lines = vec![format!(
        "Replies from others ({} total across {} posts, source={})",
        data.total_replies, data.posts.len(), data.source
    )];
    for post in &data.posts {
        let text_preview = post
            .post_text
            .as_deref()
            .unwrap_or("")
            .chars()
            .take(60)
            .collect::<String>();
        lines.push(format!("  Post [{}]: {}", post.post_id, text_preview));
        for reply in &post.replies {
            let username = reply.username.as_deref().unwrap_or("unknown");
            let reply_text = reply
                .text
                .as_deref()
                .unwrap_or("")
                .chars()
                .take(80)
                .collect::<String>();
            lines.push(format!("    @{}: {}", username, reply_text));
        }
    }
    lines.join("\n")
}
