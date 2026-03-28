pub mod account;
pub mod activity;
pub mod attempts;
pub mod auth;
pub mod config;
pub mod doctor;
pub mod log;
pub mod me;
pub mod post;
pub mod publish;
pub mod search;
pub mod validation;

use clap::{Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "threads-cli", version, about = "Threads API Rust CLI")]
pub struct Cli {
    /// Path to TOML config file
    #[arg(long, global = true)]
    pub config: Option<String>,
    /// Emit JSON output
    #[arg(long, global = true, default_value_t = false)]
    pub json: bool,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    Auth(AuthCommand),
    Account(AccountCommand),
    Publish(PublishCommand),
    Post(PostCommand),
    Log(LogCommand),
    Attempts(AttemptsCommand),
    Config(ConfigCommand),
    Doctor(DoctorCommand),
    Search(SearchCommand),
    Me(MeCommand),
    Activity(ActivityCommand),
}

#[derive(Debug, Args)]
pub struct AuthCommand {
    #[command(subcommand)]
    pub command: auth::AuthSubcommand,
}

#[derive(Debug, Args)]
pub struct AccountCommand {
    #[command(subcommand)]
    pub command: account::AccountSubcommand,
}

#[derive(Debug, Args)]
pub struct PublishCommand {
    #[command(subcommand)]
    pub command: publish::PublishSubcommand,
}

#[derive(Debug, Args)]
pub struct PostCommand {
    #[command(subcommand)]
    pub command: post::PostSubcommand,
}

#[derive(Debug, Args)]
pub struct LogCommand {
    #[command(subcommand)]
    pub command: log::LogSubcommand,
}

#[derive(Debug, Args)]
pub struct AttemptsCommand {
    #[command(subcommand)]
    pub command: attempts::AttemptsSubcommand,
}

#[derive(Debug, Args)]
pub struct ConfigCommand {
    #[command(subcommand)]
    pub command: config::ConfigSubcommand,
}

#[derive(Debug, Args)]
pub struct DoctorCommand {
    #[command(subcommand)]
    pub command: doctor::DoctorSubcommand,
}

#[derive(Debug, Args)]
pub struct SearchCommand {
    #[command(subcommand)]
    pub command: search::SearchSubcommand,
}

#[derive(Debug, Args)]
pub struct MeCommand {
    #[command(subcommand)]
    pub command: me::MeSubcommand,
}

#[derive(Debug, Args)]
pub struct ActivityCommand {
    #[command(subcommand)]
    pub command: activity::ActivitySubcommand,
}
