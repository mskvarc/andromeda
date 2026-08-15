//! Lean command-line entry point for the Andromeda broker.

#![deny(missing_docs)]

mod cli;

use crate::cli::{Cli, Command, ConfigCommand, OutputFormat};
use andromeda_broker_composition::config::runtime::{
    BrokerRunOverrides,
    ConfigurationLoadOutcome,
    ConfigurationOutputFormat,
    load_reported,
    print_effective_reported,
    validate_reported,
};
use clap::Parser;
use mimalloc::MiMalloc;
use std::process::ExitCode;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

#[tokio::main]
async fn main() -> ExitCode {
    let outcome = match Cli::parse().command {
        Command::Run(run_args) => {
            let overrides = BrokerRunOverrides {
                host: run_args.host,
                port: run_args.port,
            };
            match load_reported(&run_args.common.config_file, overrides) {
                ConfigurationLoadOutcome::Ready(config) => andromeda_broker_composition::run(*config).await,
                ConfigurationLoadOutcome::Failed(outcome) => outcome,
            }
        }
        Command::Config(config_args) => match config_args.command {
            ConfigCommand::Validate => validate_reported(&config_args.common.config_file),
            ConfigCommand::PrintEffective(print_args) => {
                let format = match print_args.format {
                    OutputFormat::Json => ConfigurationOutputFormat::Json,
                    OutputFormat::Toml => ConfigurationOutputFormat::Toml,
                };
                print_effective_reported(&config_args.common.config_file, format)
            }
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
