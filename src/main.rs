//! Lean command-line entry point for the Andromeda broker.

#![deny(missing_docs)]

mod cli;

use crate::cli::{Cli, Command, ConfigurationCommand, OutputFormat};
use andromeda_broker_composition::configuration::commands::{
    ConfigurationLoadOutcome,
    ConfigurationOutputFormat,
    generate_descriptor_reported,
    generate_schema_reported,
    generate_template_reported,
    load_reported,
    print_effective_reported,
    validate_reported,
};
use andromeda_configuration::load::ListenerOverrides;
use clap::Parser;
#[cfg(target_env = "msvc")]
use mimalloc::MiMalloc;
use std::process::ExitCode;
#[cfg(not(target_env = "msvc"))]
use tikv_jemallocator::Jemalloc;

#[cfg(not(target_env = "msvc"))]
#[global_allocator]
static GLOBAL: Jemalloc = Jemalloc;

#[cfg(target_env = "msvc")]
#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    let configuration_file = cli.configuration_file;
    let outcome = match cli.command {
        None => {
            let overrides = ListenerOverrides {
                host: cli.host,
                port: cli.port,
            };
            match load_reported(configuration_file, overrides) {
                ConfigurationLoadOutcome::Ready(loaded) => andromeda_broker_composition::run(*loaded).await,
                ConfigurationLoadOutcome::Failed(outcome) => outcome,
            }
        }
        Some(Command::Configuration(configuration_args)) => match configuration_args.command {
            ConfigurationCommand::Validate => validate_reported(configuration_file),
            ConfigurationCommand::Show(show_args) => {
                let format = match show_args.format {
                    OutputFormat::Json => ConfigurationOutputFormat::Json,
                    OutputFormat::Toml => ConfigurationOutputFormat::Toml,
                };
                print_effective_reported(configuration_file, format)
            }
            ConfigurationCommand::Generate(output_args) => generate_template_reported(output_args.output),
            ConfigurationCommand::Schema(output_args) => generate_schema_reported(output_args.output),
            ConfigurationCommand::Describe(output_args) => generate_descriptor_reported(output_args.output),
        },
    };
    ExitCode::from(outcome)
}

#[cfg(test)]
mod tests {
    use andromeda_broker_composition::outcome::ProcessExitOutcome;
    use std::{fs, io::Error as IoError, process::ExitCode};

    #[test]
    fn owner_produced_success_maps_to_process_success() {
        pretty_assertions::assert_eq!(ExitCode::from(ProcessExitOutcome::Success), ExitCode::SUCCESS);
    }

    #[test]
    fn every_owner_produced_failure_maps_to_process_failure() {
        for outcome in [
            ProcessExitOutcome::ConfigurationFailure,
            ProcessExitOutcome::StartupFailure,
            ProcessExitOutcome::RuntimeFailure,
        ] {
            pretty_assertions::assert_eq!(ExitCode::from(outcome), ExitCode::FAILURE);
        }
    }

    #[test]
    fn binary_source_directory_contains_only_cli_and_entry_point() -> Result<(), IoError> {
        let mut source_files = fs::read_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/src"))?
            .map(|entry| entry.map(|entry| entry.file_name()))
            .collect::<Result<Vec<_>, _>>()?;
        source_files.sort();
        pretty_assertions::assert_eq!(source_files, ["cli.rs", "main.rs"]);
        Ok(())
    }
}
