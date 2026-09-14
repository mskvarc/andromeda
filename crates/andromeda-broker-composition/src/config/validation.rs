//! Runtime configuration validation and path normalization.

use crate::config::{error::ConfigurationError, runtime::RuntimeConfig};
use std::{
    fmt::Display,
    path::{Path, PathBuf},
};

pub(super) fn validate_config(config: &RuntimeConfig) -> Result<(), ConfigurationError> {
    config.server.validate().map_err(owner_configuration_error)?;
    config.http.validate().map_err(owner_configuration_error)?;
    config.http3.activation().map_err(owner_configuration_error)?;
    config.database.validate().map_err(owner_configuration_error)?;
    config.jsonld.validate().map_err(owner_configuration_error)?;
    config.temporal.validate().map_err(owner_configuration_error)?;
    config.tenancy.validate().map_err(owner_configuration_error)?;
    config.entity.validate().map_err(owner_configuration_error)?;
    config.federation.validate().map_err(owner_configuration_error)?;
    config.snapshot.validate().map_err(owner_configuration_error)?;
    config.notifications.validate().map_err(owner_configuration_error)?;
    config.spatial.validate().map_err(owner_configuration_error)?;
    Ok(())
}

fn owner_configuration_error(error: impl Display) -> ConfigurationError {
    ConfigurationError::invalid(error.to_string())
}

pub(super) fn normalize_preload_context_paths(config: &mut RuntimeConfig, config_file: Option<&Path>) {
    // Relative preload paths resolve against the configuration file's directory;
    // with no file they resolve against the current working directory.
    let config_dir = config_file.and_then(Path::parent).map_or_else(|| PathBuf::from("."), PathBuf::from);
    config.jsonld.resolve_preload_paths(&config_dir);
}
