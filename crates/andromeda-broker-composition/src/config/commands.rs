//! The `run` and `configuration` commands' use of the configuration: load it,
//! admit its sections, and report the outcome to the operator.

use crate::{
    config::{error::ConfigurationError, section_admission::admit_sections},
    outcome::ProcessExitOutcome,
};
use andromeda_configuration::{
    broker_config::BrokerConfig,
    load::{ListenerOverrides, load, resolve_config_file},
    redact::{redacted_json, redacted_toml},
    template::default_template,
};
use std::{
    fs,
    path::{Path, PathBuf},
};

/// Effective configuration output selected by the CLI.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConfigurationOutputFormat {
    /// Pretty JSON output.
    Json,
    /// Pretty TOML output.
    Toml,
}

/// Result of configuration loading after its failures have been reported.
pub enum ConfigurationLoadOutcome {
    /// An admitted configuration is ready for broker composition.
    Ready(Box<BrokerConfig>),
    /// Loading failed and the contained process outcome is ready to return.
    Failed(ProcessExitOutcome),
}

/// The configuration file that contributed, if any, and the configuration it,
/// the environment and `overrides` load to, with every section admitted.
///
/// # Errors
///
/// Returns [`ConfigurationError::Load`] when the configuration does not load
/// and [`ConfigurationError::Admission`] when a domain refuses a section value.
pub(crate) fn load_admitted(config_file: Option<PathBuf>, overrides: ListenerOverrides) -> Result<(Option<PathBuf>, BrokerConfig), ConfigurationError> {
    let config_file = resolve_config_file(config_file)?;
    let config = load(config_file.as_deref(), overrides)?;
    admit_sections(&config)?;
    Ok((config_file, config))
}

/// Load the configuration for broker execution, reporting a failure.
#[must_use]
pub fn load_reported(config_file: Option<PathBuf>, overrides: ListenerOverrides) -> ConfigurationLoadOutcome {
    match load_admitted(config_file, overrides) {
        Ok((_, config)) => ConfigurationLoadOutcome::Ready(Box::new(config)),
        Err(error) => {
            eprintln!("{error}");
            ConfigurationLoadOutcome::Failed(ProcessExitOutcome::ConfigurationFailure)
        }
    }
}

/// Validate the configuration and report the result.
#[must_use]
pub fn validate_reported(config_file: Option<PathBuf>) -> ProcessExitOutcome {
    match load_admitted(config_file, ListenerOverrides::default()) {
        Ok((Some(path), _)) => {
            println!("Configuration valid: {}", path.display());
            ProcessExitOutcome::Success
        }
        Ok((None, _)) => {
            println!("Configuration valid: defaults and environment (no configuration file present)");
            ProcessExitOutcome::Success
        }
        Err(error) => {
            eprintln!("{error}");
            ProcessExitOutcome::ConfigurationFailure
        }
    }
}

/// Print the effective redacted configuration, reporting a failure.
#[must_use]
pub fn print_effective_reported(config_file: Option<PathBuf>, format: ConfigurationOutputFormat) -> ProcessExitOutcome {
    let rendered = load_admitted(config_file, ListenerOverrides::default()).and_then(|(_, config)| {
        Ok(match format {
            ConfigurationOutputFormat::Json => redacted_json(&config)?,
            ConfigurationOutputFormat::Toml => redacted_toml(&config)?,
        })
    });
    match rendered {
        Ok(rendered) => {
            println!("{rendered}");
            ProcessExitOutcome::Success
        }
        Err(error) => {
            eprintln!("{error}");
            ProcessExitOutcome::ConfigurationFailure
        }
    }
}

/// Emit a fully commented default configuration, reporting a write failure.
///
/// With `output` set the commented TOML is written to that file; without it the
/// document is printed to standard output. Every emitted setting carries the
/// rustdoc of the field it comes from as a leading comment, and every field
/// left at its built-in default is shown with that default's value.
#[must_use]
pub fn generate_default_reported(output: Option<PathBuf>) -> ProcessExitOutcome {
    let rendered = default_template();
    let Some(path) = output else {
        print!("{rendered}");
        return ProcessExitOutcome::Success;
    };
    write_template(&path, &rendered)
}

/// Write the template to `path`, reporting the outcome.
fn write_template(path: &Path, rendered: &str) -> ProcessExitOutcome {
    match fs::write(path, rendered) {
        Ok(()) => {
            println!("Default configuration written to {}", path.display());
            ProcessExitOutcome::Success
        }
        Err(error) => {
            eprintln!("failed to write default configuration to {}: {error}", path.display());
            ProcessExitOutcome::ConfigurationFailure
        }
    }
}
