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

use crate::{
    broker_config::BrokerConfig,
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
const CONFIG_SUBDIR: &str = "andromeda";

/// Basename of the broker configuration file within the configuration directory.
const CONFIG_FILE_NAME: &str = "andromeda.toml";

/// The prefix every environment variable the configuration reads carries.
const ENVIRONMENT_PREFIX: &str = "ANDROMEDA__";

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
    /// A `--config` path names a file the operator expects to be read, so a
    /// missing one is a hard error; the default configuration file, by
    /// contrast, is loaded only when present.
    #[error("configuration file not found: {0}")]
    MissingFile(PathBuf),
    /// The layers could not be read into the configuration: a value of the
    /// wrong type, an unknown key, or a file that is not TOML. Boxed: it is by
    /// far the largest variant.
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
pub fn default_config_file() -> Option<PathBuf> {
    let strategy = choose_base_strategy().ok()?;
    Some(strategy.config_dir().join(CONFIG_SUBDIR).join(CONFIG_FILE_NAME))
}

/// Which configuration file, if any, contributes a file layer.
///
/// An explicit `--config` path is honored verbatim and must exist. Without one,
/// the default file is used only when it is present.
///
/// # Errors
///
/// Returns [`LoadError::MissingFile`] for an explicit path that does not exist.
pub fn resolve_config_file(explicit: Option<PathBuf>) -> Result<Option<PathBuf>, LoadError> {
    match explicit {
        Some(path) if path.exists() => Ok(Some(path)),
        Some(path) => Err(LoadError::MissingFile(path)),
        None => Ok(default_config_file().filter(|path| path.exists())),
    }
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
pub fn load(config_file: Option<&Path>, overrides: ListenerOverrides) -> Result<BrokerConfig, LoadError> {
    let mut figment = Figment::from(Serialized::defaults(BrokerConfig::default()));
    if let Some(path) = config_file {
        figment = figment.merge(Toml::file(path));
    }
    figment = figment.merge(Env::prefixed(ENVIRONMENT_PREFIX).split("__").parser(environment_scalar));
    if let Some(host) = overrides.host {
        figment = figment.merge(Serialized::default("server.host", host));
    }
    if let Some(port) = overrides.port {
        figment = figment.merge(Serialized::default("server.port", port));
    }
    let mut config: BrokerConfig = figment.extract().map_err(|error| LoadError::Layers(Box::new(error)))?;
    // Relative preload paths resolve against the configuration file's
    // directory; with no file they resolve against the working directory.
    let config_dir = config_file.and_then(Path::parent).map_or_else(|| PathBuf::from("."), Path::to_path_buf);
    config.jsonld.resolve_preload_paths(&config_dir);
    validate(&config)?;
    Ok(config)
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
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests;
