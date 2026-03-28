use clap::Subcommand;
use serde::Serialize;

use crate::config::AppConfig;
use crate::error::CliError;
use crate::output::{print_success, OutputMode};

#[derive(Debug, Subcommand)]
pub enum ConfigSubcommand {
    Show,
}

#[derive(Serialize)]
struct ConfigShowData {
    config: crate::config::RedactedConfigView,
}

pub fn run(
    command: super::ConfigCommand,
    app: &AppConfig,
    output_mode: OutputMode,
) -> Result<(), CliError> {
    match command.command {
        ConfigSubcommand::Show => {
            let data = ConfigShowData {
                config: app.redacted_for_display(),
            };
            print_success(
                output_mode,
                "config show",
                "Config loaded successfully (sensitive values redacted).",
                data,
            );
            Ok(())
        }
    }
}
