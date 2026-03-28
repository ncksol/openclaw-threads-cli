use clap::{Args, Subcommand};
use serde::Serialize;

use crate::error::CliError;
use crate::output::{print_success, OutputMode};
use crate::store::Store;

#[derive(Debug, Subcommand)]
pub enum AttemptsSubcommand {
    List(ListArgs),
}

#[derive(Debug, Args)]
pub struct ListArgs {
    #[arg(long, default_value_t = 20)]
    pub limit: usize,
}

#[derive(Debug, Serialize)]
struct AttemptData {
    id: i64,
    attempt_uuid: String,
    kind: String,
    status: String,
    threads_post_id: Option<String>,
    error_code: Option<String>,
    error_message: Option<String>,
    created_at: String,
}

#[derive(Debug, Serialize)]
struct AttemptsListData {
    attempts: Vec<AttemptData>,
}

pub fn run(
    command: super::AttemptsCommand,
    store: &Store,
    output_mode: OutputMode,
) -> Result<(), CliError> {
    match command.command {
        AttemptsSubcommand::List(args) => {
            let rows = store.list_attempts(args.limit)?;
            let attempts = rows
                .into_iter()
                .map(|r| AttemptData {
                    id: r.id,
                    attempt_uuid: r.attempt_uuid,
                    kind: r.kind,
                    status: r.status,
                    threads_post_id: r.threads_post_id,
                    error_code: r.error_code,
                    error_message: r.error_message,
                    created_at: r.created_at,
                })
                .collect();
            let data = AttemptsListData { attempts };
            print_success(
                output_mode,
                "attempts list",
                "Listed publish attempts.",
                data,
            );
            Ok(())
        }
    }
}
