use clap::{Args, Subcommand};
use serde::Serialize;

use crate::error::{CliError, ErrorCategory};
use crate::output::{print_success, OutputMode};
use crate::store::{PostRow, Store};

#[derive(Debug, Subcommand)]
pub enum LogSubcommand {
    List(ListArgs),
    Get(GetArgs),
}

#[derive(Debug, Args)]
pub struct ListArgs {
    #[arg(long, default_value_t = 20)]
    pub limit: usize,
}

#[derive(Debug, Args)]
pub struct GetArgs {
    #[arg(long)]
    pub id: i64,
}

#[derive(Debug, Serialize)]
struct LogListData {
    records: Vec<LogRecordData>,
}

#[derive(Debug, Serialize)]
struct LogRecordData {
    local_id: i64,
    threads_post_id: String,
    kind: String,
    text: String,
    post_url: Option<String>,
    topic_tag: Option<String>,
    source_url: Option<String>,
    source_link_mode: Option<String>,
    parent_threads_post_id: Option<String>,
    published_at: Option<String>,
    created_at: String,
}

#[derive(Debug, Serialize)]
struct LogGetData {
    record: LogRecordData,
}

pub fn run(
    command: super::LogCommand,
    store: &Store,
    output_mode: OutputMode,
) -> Result<(), CliError> {
    match command.command {
        LogSubcommand::List(args) => {
            let rows = store.list_posts(args.limit)?;
            let records = rows.into_iter().map(to_log_record).collect();
            let data = LogListData { records };
            print_success(
                output_mode,
                "log list",
                "Listed local post records.",
                data,
            );
            Ok(())
        }
        LogSubcommand::Get(args) => {
            let row = store.get_post_by_local_id(args.id)?.ok_or_else(|| {
                CliError::new(
                    ErrorCategory::Validation,
                    format!("no local log record found for id={}", args.id),
                )
            })?;
            let data = LogGetData {
                record: to_log_record(row),
            };
            print_success(
                output_mode,
                "log get",
                format!("Retrieved local post record id={}", args.id),
                data,
            );
            Ok(())
        }
    }
}

fn to_log_record(row: PostRow) -> LogRecordData {
    LogRecordData {
        local_id: row.id,
        threads_post_id: row.threads_post_id,
        kind: row.kind,
        text: row.text,
        post_url: row.post_url,
        topic_tag: row.topic_tag,
        source_url: row.source_url,
        source_link_mode: row.source_link_mode,
        parent_threads_post_id: row.parent_threads_post_id,
        published_at: row.published_at,
        created_at: row.created_at,
    }
}
