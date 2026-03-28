use clap::Subcommand;
use serde::Serialize;

use crate::client::{AccountIdentity, ThreadsClient};
use crate::config::AppConfig;
use crate::error::{CliError, ErrorCategory};
use crate::output::{print_success, OutputMode};
use crate::store::{retry_with_backoff, RetryOperation, RetryPolicy, Store};

#[derive(Debug, Subcommand)]
pub enum AccountSubcommand {
    Whoami,
}

#[derive(Debug, Serialize)]
struct WhoAmIData {
    account_id: String,
    username: Option<String>,
    name: Option<String>,
}

pub async fn run(
    command: super::AccountCommand,
    app: &AppConfig,
    store: &Store,
    output_mode: OutputMode,
) -> Result<(), CliError> {
    match command.command {
        AccountSubcommand::Whoami => run_whoami(app, store, output_mode).await,
    }
}

async fn run_whoami(app: &AppConfig, store: &Store, output_mode: OutputMode) -> Result<(), CliError> {
    let token = store.latest_access_token()?.ok_or_else(|| {
        CliError::new(
            ErrorCategory::Auth,
            "no stored access token found; run auth login first",
        )
    })?;

    let client = ThreadsClient::from_config(app)?;
    let identity = retry_with_backoff(
        RetryPolicy {
            max_retries: 3,
            base_delay_ms: 100,
        },
        RetryOperation::SafeRead,
        || async { client.fetch_account_identity(&token).await },
    )
    .await?;

    print_success(
        output_mode,
        "account whoami",
        "Fetched current account identity.",
        map_identity(identity),
    );
    Ok(())
}

fn map_identity(identity: AccountIdentity) -> WhoAmIData {
    WhoAmIData {
        account_id: identity.id,
        username: identity.username,
        name: identity.name,
    }
}
