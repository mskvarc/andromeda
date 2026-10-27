#![expect(clippy::absolute_paths, reason = "shadow generates absolute standard-library paths")]

use clap::{Args, Parser, Subcommand, ValueEnum};
use shadow_rs::shadow;
use std::{net::IpAddr, path::PathBuf};

shadow!(build);

/// Andromeda NGSI-LD context broker.
///
/// With no subcommand the broker starts and serves requests. The `config`
/// subcommand inspects, validates, or generates configuration without starting
/// the broker.
#[derive(Parser)]
#[command(version = build::CLAP_LONG_VERSION, about, long_about = None)]
#[command(disable_help_subcommand = true)]
#[clap(styles = clap_themes::CLAP_V3)]
pub struct Cli {
    /// Configuration file to load. When omitted, the default file under the
    /// platform configuration directory is read if present; otherwise the
    /// broker runs from built-in defaults and the environment alone.
    #[arg(short = 'c', long = "config", value_name = "FILE", global = true)]
    pub config_file: Option<PathBuf>,

    /// Address the HTTP listener binds to, overriding the file and environment.
    #[arg(short = 'H', long, value_name = "ADDR")]
    pub host: Option<IpAddr>,

    /// Port the HTTP listener binds to, overriding the file and environment.
    #[arg(short = 'p', long, value_name = "PORT")]
    pub port: Option<u16>,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand)]
pub enum Command {
    /// Inspect, validate, or generate broker configuration.
    Config(ConfigArgs),
}

#[derive(Args)]
pub struct ConfigArgs {
    #[command(subcommand)]
    pub command: ConfigCommand,
}

#[derive(Subcommand)]
pub enum ConfigCommand {
    /// Load and validate the effective configuration, then report and exit.
    Validate,

    /// Print the effective configuration with secrets redacted.
    Show(ShowArgs),

    /// Write a fully commented configuration file populated with the defaults.
    Generate(GenerateArgs),

    /// Write the JSON Schema of the configuration file.
    Schema(OutputArgs),

    /// Write the configuration descriptor: every key with its type, default,
    /// choices, environment variable and limits, as JSON.
    Describe(OutputArgs),
}

#[derive(Args)]
pub struct ShowArgs {
    /// Output format for the redacted effective configuration.
    #[arg(short = 'f', long, value_enum, default_value_t = OutputFormat::Toml)]
    pub format: OutputFormat,
}

#[derive(Args)]
pub struct GenerateArgs {
    /// File to write the commented configuration to. Printed to standard output
    /// when omitted.
    #[arg(short = 'o', long, value_name = "FILE")]
    pub output: Option<PathBuf>,
}

#[derive(Args)]
pub struct OutputArgs {
    /// File to write to. Printed to standard output when omitted.
    #[arg(short = 'o', long, value_name = "FILE")]
    pub output: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug, Default, ValueEnum)]
pub enum OutputFormat {
    Json,
    #[default]
    Toml,
}
