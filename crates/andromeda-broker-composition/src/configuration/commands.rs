//! The `run` and `configuration` commands' use of the configuration: load it,
//! admit its sections, and report the outcome to the operator.

use crate::{
    configuration::{error::ConfigurationError, section_admission::admit_sections},
    outcome::ProcessExitOutcome,
};
use andromeda_configuration::{
    broker_config::BrokerConfig,
    load::{ListenerOverrides, load, resolve_configuration_file},
    redact::{redacted_json, redacted_toml},
    reference::{ReferenceError, descriptor::configuration_descriptor_text, json_schema::configuration_schema_text, template::configuration_template},
};
use std::{fs, path::PathBuf};

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
pub(crate) fn load_admitted(configuration_file: Option<PathBuf>, overrides: ListenerOverrides) -> Result<(Option<PathBuf>, BrokerConfig), ConfigurationError> {
    let configuration_file = resolve_configuration_file(configuration_file)?;
    let config = load(configuration_file.as_deref(), overrides)?;
    admit_sections(&config)?;
    Ok((configuration_file, config))
}

/// Load the configuration for broker execution, reporting a failure.
#[must_use]
pub fn load_reported(configuration_file: Option<PathBuf>, overrides: ListenerOverrides) -> ConfigurationLoadOutcome {
    match load_admitted(configuration_file, overrides) {
        Ok((_, config)) => ConfigurationLoadOutcome::Ready(Box::new(config)),
        Err(error) => {
            eprintln!("{error}");
            ConfigurationLoadOutcome::Failed(ProcessExitOutcome::ConfigurationFailure)
        }
    }
}

/// Validate the configuration and report the result.
#[must_use]
pub fn validate_reported(configuration_file: Option<PathBuf>) -> ProcessExitOutcome {
    match load_admitted(configuration_file, ListenerOverrides::default()) {
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
pub fn print_effective_reported(configuration_file: Option<PathBuf>, format: ConfigurationOutputFormat) -> ProcessExitOutcome {
    let rendered = load_admitted(configuration_file, ListenerOverrides::default()).and_then(|(_, config)| {
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

/// Emit the commented configuration template, reporting a failure.
///
/// With `output` set the template is written to that file; without it the
/// document is printed to standard output. Every table and key carries its
/// operator description as a leading comment and is shown at its built-in
/// default; a key that is not set by default is shown commented out, at an
/// example value.
#[must_use]
pub fn generate_template_reported(output: Option<PathBuf>) -> ProcessExitOutcome {
    emit_reported(configuration_template(), output, "configuration template")
}

/// Emit the JSON Schema of the configuration file, reporting a failure.
///
/// With `output` set the schema is written to that file; without it the schema
/// is printed to standard output.
#[must_use]
pub fn generate_schema_reported(output: Option<PathBuf>) -> ProcessExitOutcome {
    emit_reported(configuration_schema_text(), output, "configuration schema")
}

/// Emit the configuration descriptor, reporting a failure.
///
/// With `output` set the descriptor is written to that file; without it the
/// descriptor is printed to standard output.
#[must_use]
pub fn generate_descriptor_reported(output: Option<PathBuf>) -> ProcessExitOutcome {
    emit_reported(configuration_descriptor_text(), output, "configuration descriptor")
}

/// Print `rendered`, or write it to `output`, reporting the outcome; `what`
/// names the document in the report.
fn emit_reported(rendered: Result<String, ReferenceError>, output: Option<PathBuf>, what: &str) -> ProcessExitOutcome {
    let rendered = match rendered {
        Ok(rendered) => rendered,
        Err(error) => {
            eprintln!("{error}");
            return ProcessExitOutcome::ConfigurationFailure;
        }
    };
    let Some(path) = output else {
        print!("{rendered}");
        return ProcessExitOutcome::Success;
    };
    match fs::write(&path, rendered) {
        Ok(()) => {
            println!("{what} written to {}", path.display());
            ProcessExitOutcome::Success
        }
        Err(error) => {
            eprintln!("failed to write the {what} to {}: {error}", path.display());
            ProcessExitOutcome::ConfigurationFailure
        }
    }
}
