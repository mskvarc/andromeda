//! Finding, layering and reading the configuration.
//!
//! The layers, in increasing precedence, are the built-in defaults, the TOML
//! file, the `ANDROMEDA__`-prefixed environment, and the command-line listener
//! overrides. An environment variable nests with `__` between every level, so
//! `ANDROMEDA__NOTIFICATIONS__PERIODIC__MAX_POLL_INTERVAL` sets
//! `notifications.periodic.max_poll_interval`; a single underscore separates the words
//! of one key and never nests. An environment variable sets a scalar only: a
//! list setting takes its members separated by commas, and a table is set in
//! the file alone.
//!
//! A key the configuration does not define is ignored and reported, so a file
//! written for a later version still loads. A defined key holding a value it
//! does not define is refused.

pub mod ignored_setting;

use crate::{
    broker_config::BrokerConfig,
    load::ignored_setting::{IgnoredSetting, WritingLayers, ignored_settings},
    validate::{InvalidConfiguration, validate},
};
use etcetera::{BaseStrategy, choose_base_strategy};
use figment2::{
    Error as LayerError,
    Figment,
    providers::{Env, Format, Serialized, Toml},
    value::Value,
};
use std::{
    net::IpAddr,
    path::{Path, PathBuf},
};
use thiserror::Error;

/// Application subdirectory holding Andromeda's files under a base directory.
const CONFIGURATION_SUBDIRECTORY: &str = "andromeda";

/// Basename of the broker configuration file within the configuration directory.
const CONFIGURATION_FILE_NAME: &str = "andromeda.toml";

/// The prefix every environment variable the configuration reads carries.
pub(crate) const ENVIRONMENT_PREFIX: &str = "ANDROMEDA__";

/// The layers in the order a later one overrides an earlier one, for operator
/// reporting.
pub const PRECEDENCE: &str = "CLI host/port > environment > config file > defaults";

/// Command-line listener overrides applied above every other layer.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ListenerOverrides {
    /// Listener address override.
    pub host: Option<IpAddr>,
    /// Listener port override.
    pub port: Option<u16>,
}

/// The configuration could not be loaded.
#[derive(Debug, Error)]
pub enum LoadError {
    /// An explicitly requested configuration file does not exist.
    ///
    /// A `--configuration` path names a file the operator expects to be read, so a
    /// missing one is a hard error; the default configuration file, by
    /// contrast, is loaded only when present.
    #[error("configuration file not found: {0}")]
    MissingFile(PathBuf),
    /// The layers could not be read into the configuration: a value of the
    /// wrong type, a choice a key does not define, or a file that is not
    /// TOML. Boxed: it is by far the largest variant.
    #[error("loading configuration failed: {0}")]
    Layers(#[source] Box<LayerError>),
    /// The configuration was read, and a check refused it.
    #[error(transparent)]
    Invalid(#[from] InvalidConfiguration),
}

/// The default broker configuration file under the platform configuration
/// directory.
///
/// The location follows platform convention through the XDG base-directory
/// strategy: `$XDG_CONFIG_HOME/andromeda/andromeda.toml` (falling back to
/// `~/.config/andromeda/andromeda.toml`) on Unix, `~/Library/Application
/// Support` on macOS, and `%APPDATA%` on Windows. No file is shipped, so the
/// path is read only when it exists. `None` when no home directory can be
/// determined, in which case the broker runs from defaults and environment
/// alone.
#[must_use]
pub fn default_configuration_file() -> Option<PathBuf> {
    let strategy = choose_base_strategy().ok()?;
    Some(strategy.config_dir().join(CONFIGURATION_SUBDIRECTORY).join(CONFIGURATION_FILE_NAME))
}

/// Which configuration file, if any, contributes a file layer.
///
/// An explicit `--configuration` path is honored verbatim and must exist. Without one,
/// the default file is used only when it is present.
///
/// # Errors
///
/// Returns [`LoadError::MissingFile`] for an explicit path that does not exist.
pub fn resolve_configuration_file(explicit: Option<PathBuf>) -> Result<Option<PathBuf>, LoadError> {
    match explicit {
        Some(path) if path.exists() => Ok(Some(path)),
        Some(path) => Err(LoadError::MissingFile(path)),
        None => Ok(default_configuration_file().filter(|path| path.exists())),
    }
}

/// A configuration that loaded, and the settings its layers write that it
/// does not define.
#[derive(Debug)]
pub struct LoadedConfiguration {
    /// The configuration in effect.
    pub config: BrokerConfig,
    /// Every key the file or the environment writes that the configuration
    /// does not define, once for each layer that writes it.
    pub ignored: Vec<IgnoredSetting>,
}

/// Load, normalize and check the configuration.
///
/// A `None` file means no file layer contributes: the broker loads from
/// defaults and environment alone.
///
/// # Errors
///
/// Returns [`LoadError::Layers`] when the layers do not read into the
/// configuration, and [`LoadError::Invalid`] when a check refuses it.
pub fn load(configuration_file: Option<&Path>, overrides: ListenerOverrides) -> Result<LoadedConfiguration, LoadError> {
    let mut figment = Figment::from(Serialized::defaults(BrokerConfig::default()));
    if let Some(path) = configuration_file {
        figment = figment.merge(Toml::file(path));
    }
    figment = figment.merge(environment_layer());
    if let Some(host) = overrides.host {
        figment = figment.merge(Serialized::default("server.host", host));
    }
    if let Some(port) = overrides.port {
        figment = figment.merge(Serialized::default("server.port", port));
    }
    let mut config: BrokerConfig = figment.extract().map_err(|error| LoadError::Layers(Box::new(error)))?;
    let writing_layers = WritingLayers {
        file: configuration_file.map(|path| (path, Figment::from(Toml::file(path)))),
        environment: Figment::from(environment_layer()),
    };
    let ignored = ignored_settings(&figment, &writing_layers).map_err(LoadError::Layers)?;
    // Relative preload paths resolve against the configuration file's
    // directory; with no file they resolve against the working directory.
    let configuration_directory = configuration_file.and_then(Path::parent).map_or_else(|| PathBuf::from("."), Path::to_path_buf);
    config.jsonld.resolve_preload_paths(&configuration_directory);
    validate(&config)?;
    Ok(LoadedConfiguration { config, ignored })
}

/// The `ANDROMEDA__` environment layer: `__` nests, and every value is a
/// scalar.
fn environment_layer() -> Env {
    Env::prefixed(ENVIRONMENT_PREFIX).split("__").parser(environment_scalar)
}

/// The scalar an environment value spells: a boolean, an integer, a decimal
/// number, or otherwise the text itself. Never a table or an array; a list
/// setting reads its comma-separated members from the text.
fn environment_scalar(raw: &str) -> Value {
    match raw {
        "true" => Value::from(true),
        "false" => Value::from(false),
        _ => raw
            .parse::<i64>()
            .map(Value::from)
            .ok()
            .or_else(|| {
                raw.contains('.')
                    .then(|| raw.parse::<f64>().ok().filter(|number| number.is_finite()))
                    .flatten()
                    .map(Value::from)
            })
            .unwrap_or_else(|| Value::from(raw)),
    }
}

#[cfg(test)]
mod tests;
