mod cli;
mod client;
mod config;
mod error;
mod output;
mod store;
mod tracing_setup;

use anyhow::Result;
use clap::Parser;
use cli::{Cli, Command};
use output::OutputMode;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_setup::init_tracing()?;

    let cli = Cli::parse();
    let output_mode = if cli.json { OutputMode::Json } else { OutputMode::Human };
    let command_name = command_name(&cli.command);

    let app = match config::AppConfig::load(cli.config.as_deref()) {
        Ok(cfg) => cfg,
        Err(err) => return output::print_error_and_exit(output_mode, command_name, err.into()),
    };

    let store = match store::Store::open(&app.storage.database_path) {
        Ok(store) => store,
        Err(err) => return output::print_error_and_exit(output_mode, command_name, err.into()),
    };

    if let Err(err) = store.run_migrations() {
        return output::print_error_and_exit(output_mode, command_name, err.into());
    }

    let result = match cli.command {
        Command::Config(cmd) => cli::config::run(cmd, &app, output_mode),
        Command::Doctor(cmd) => cli::doctor::run(cmd, &app, &store, output_mode),
        Command::Auth(cmd) => cli::auth::run(cmd, &app, &store, output_mode).await,
        Command::Account(cmd) => cli::account::run(cmd, &app, &store, output_mode).await,
        Command::Publish(cmd) => cli::publish::run(cmd, &app, &store, output_mode),
        Command::Post(cmd) => cli::post::run(cmd, &app, &store, output_mode).await,
        Command::Log(cmd) => cli::log::run(cmd, &store, output_mode),
        Command::Attempts(cmd) => cli::attempts::run(cmd, &store, output_mode),
    };

    if let Err(err) = result {
        return output::print_error_and_exit(output_mode, command_name, err);
    }

    Ok(())
}

fn command_name(command: &Command) -> &'static str {
    match command {
        Command::Config(_) => "config",
        Command::Doctor(_) => "doctor",
        Command::Auth(_) => "auth",
        Command::Account(_) => "account",
        Command::Publish(_) => "publish",
        Command::Post(_) => "post",
        Command::Log(_) => "log",
        Command::Attempts(_) => "attempts",
    }
}
