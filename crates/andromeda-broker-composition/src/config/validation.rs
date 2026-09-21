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
    config.extensions.validate().map_err(owner_configuration_error)?;
    validate_acquire_timeout_below_request_timeout(config)?;
    Ok(())
}

/// The connection acquire deadline must be strictly below the HTTP request
/// timeout.
///
/// A cross-section invariant no single config owner can check, because it spans
/// `database` and `http`. If the pool waited as long as (or longer than) the
/// request deadline for a connection, a saturated pool would race the request
/// timeout and surface as a stalled 500 rather than the fast 503 with
/// `Retry-After` the acquire deadline exists to produce.
fn validate_acquire_timeout_below_request_timeout(config: &RuntimeConfig) -> Result<(), ConfigurationError> {
    let acquire = config.database.acquire_timeout_secs;
    let request = config.http.request_timeout.duration().as_secs();
    if acquire >= request {
        return Err(ConfigurationError::invalid(format!(
            "database.acquire_timeout_secs ({acquire}) must be below http.request_timeout ({request}s) so a saturated pool answers 503 before the request deadline"
        )));
    }
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

#[cfg(test)]
mod tests {
    use crate::config::{runtime::RuntimeConfig, validation::validate_acquire_timeout_below_request_timeout};
    use andromeda_http_stack::config::human_duration::HumanDuration;

    #[test]
    fn acquire_timeout_must_be_below_the_request_timeout() {
        let mut config = RuntimeConfig::default();
        config.http.request_timeout = HumanDuration::from_secs(10);

        config.database.acquire_timeout_secs = 9;
        assert!(
            validate_acquire_timeout_below_request_timeout(&config).is_ok(),
            "an acquire below the request timeout is accepted"
        );

        config.database.acquire_timeout_secs = 10;
        assert!(
            validate_acquire_timeout_below_request_timeout(&config).is_err(),
            "an acquire equal to the request timeout is rejected"
        );

        config.database.acquire_timeout_secs = 11;
        assert!(
            validate_acquire_timeout_below_request_timeout(&config).is_err(),
            "an acquire above the request timeout is rejected"
        );
    }
}
