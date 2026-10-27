#![expect(clippy::absolute_paths, reason = "shadow generates absolute standard-library paths")]

use clap::{Args, Parser, Subcommand, ValueEnum};
use shadow_rs::shadow;
use std::{net::IpAddr, path::PathBuf};

shadow!(build);

/// Andromeda NGSI-LD context broker.
///
/// With no subcommand the broker starts and serves requests. The
/// `configuration` subcommand inspects, validates, or generates configuration
/// without starting the broker.
#[derive(Parser)]
#[command(version = build::CLAP_LONG_VERSION, about, long_about = None)]
#[command(disable_help_subcommand = true)]
#[clap(styles = clap_themes::CLAP_V3)]
pub struct Cli {
    /// Configuration file to load. When omitted, the default file under the
    /// platform configuration directory is read if present; otherwise the
    /// broker runs from built-in defaults and the environment alone.
    #[arg(short = 'c', long = "configuration", alias = "config", value_name = "FILE", global = true)]
    pub configuration_file: Option<PathBuf>,

    /// Address the HTTP listener binds to, overriding the file and environment.
    #[arg(short = 'H', long, value_name = "ADDRESS")]
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
    #[command(alias = "config")]
    Configuration(ConfigurationArgs),
}

#[derive(Args)]
pub struct ConfigurationArgs {
    #[command(subcommand)]
    pub command: ConfigurationCommand,
}

#[derive(Subcommand)]
pub enum ConfigurationCommand {
    /// Load and validate the effective configuration, then report and exit.
    Validate,

    /// Print the effective configuration with secrets redacted.
    Show(ShowArgs),

    /// Write a fully commented configuration file populated with the defaults.
    Generate(OutputArgs),

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

#[cfg(test)]
mod tests {
    use crate::cli::{Cli, Command, ConfigurationArgs, ConfigurationCommand, OutputArgs};
    use clap::Parser;
    use pretty_assertions::assert_eq;
    use std::path::PathBuf;

    fn subcommand(arguments: &[&str]) -> Result<Option<ConfigurationCommand>, clap::Error> {
        Ok(Cli::try_parse_from(arguments)?
            .command
            .map(|Command::Configuration(ConfigurationArgs { command })| command))
    }

    /// The help text `arguments` print, split into words.
    fn help_words(arguments: &[&str]) -> Vec<String> {
        let help = Cli::try_parse_from(arguments).err().map(|error| error.to_string()).unwrap_or_default();
        help.split(|character: char| character.is_whitespace() || matches!(character, ',' | '[' | ']' | '<' | '>'))
            .map(str::to_owned)
            .collect()
    }

    #[test]
    fn configuration_validate_and_config_validate_parse_to_the_same_command() -> Result<(), clap::Error> {
        assert!(matches!(
            subcommand(&["andromeda", "configuration", "validate"])?,
            Some(ConfigurationCommand::Validate)
        ));
        assert!(matches!(
            subcommand(&["andromeda", "config", "validate"])?,
            Some(ConfigurationCommand::Validate)
        ));
        Ok(())
    }

    #[test]
    fn the_configuration_flag_and_its_config_alias_set_the_same_path() -> Result<(), clap::Error> {
        let file = Some(PathBuf::from("andromeda.toml"));
        for arguments in [
            ["andromeda", "--configuration", "andromeda.toml", "configuration", "validate"],
            ["andromeda", "--config", "andromeda.toml", "configuration", "validate"],
            ["andromeda", "configuration", "validate", "--config", "andromeda.toml"],
            ["andromeda", "-c", "andromeda.toml", "configuration", "validate"],
        ] {
            assert_eq!(Cli::try_parse_from(arguments)?.configuration_file, file);
        }
        Ok(())
    }

    #[test]
    fn configuration_schema_takes_an_output_file() -> Result<(), clap::Error> {
        let parsed = subcommand(&["andromeda", "configuration", "schema", "--output", "andromeda.schema.json"])?;
        assert!(matches!(parsed, Some(ConfigurationCommand::Schema(OutputArgs { output: Some(path) })) if path.as_os_str() == "andromeda.schema.json"));
        Ok(())
    }

    #[test]
    fn help_shows_neither_config_alias() {
        for arguments in [["andromeda", "--help"].as_slice(), ["andromeda", "configuration", "--help"].as_slice()] {
            let words = help_words(arguments);
            assert!(words.iter().any(|word| word == "--configuration"), "{arguments:?} shows the flag");
            assert!(!words.iter().any(|word| word == "config" || word == "--config"), "{arguments:?} shows an alias");
        }
        let top_level = help_words(&["andromeda", "--help"]);
        assert!(top_level.iter().any(|word| word == "configuration"), "the subcommand is listed");
    }
}
