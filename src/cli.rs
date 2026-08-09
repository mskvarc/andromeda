use crate::config::runtime::DEFAULT_CONFIG_PATH;
use clap::{Args, Parser, Subcommand, ValueEnum};
use shadow_rs::shadow;
use std::{net::IpAddr, path::PathBuf};

shadow!(build);

#[derive(Parser)]
#[command(version = build::CLAP_LONG_VERSION, about, long_about = None)]
#[command(disable_help_subcommand = true)]
#[clap(styles = clap_themes::CLAP_V3)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    Run(RunArgs),
    Config(ConfigArgs),
}

#[derive(Args)]
pub struct CommonConfigArgs {
    #[arg(short = 'c', long = "config", default_value = DEFAULT_CONFIG_PATH)]
    pub config_file: PathBuf,
}

#[derive(Args)]
pub struct RunArgs {
    #[command(flatten)]
    pub common: CommonConfigArgs,

    #[arg(short = 'H', long)]
    pub host: Option<IpAddr>,

    #[arg(short = 'p', long)]
    pub port: Option<u16>,
}

#[derive(Args)]
pub struct ConfigArgs {
    #[command(flatten)]
    pub common: CommonConfigArgs,

    #[command(subcommand)]
    pub command: ConfigCommand,
}

#[derive(Subcommand)]
pub enum ConfigCommand {
    Validate,
    PrintEffective(PrintEffectiveArgs),
}

#[derive(Args)]
pub struct PrintEffectiveArgs {
    #[arg(long, value_enum, default_value_t = OutputFormat::Toml)]
    pub format: OutputFormat,
}

#[derive(Clone, Copy, Debug, Default, ValueEnum)]
pub enum OutputFormat {
    Json,
    #[default]
    Toml,
}
