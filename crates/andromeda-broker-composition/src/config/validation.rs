//! Runtime configuration validation and path normalization.

use crate::config::{
    error::ConfigurationError,
    management::{ManagementConfigError, ManagementMode},
    runtime::RuntimeConfig,
};
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
    config.management.validate().map_err(owner_configuration_error)?;
    config.extensions.validate().map_err(owner_configuration_error)?;
    validate_acquire_timeout_below_request_timeout(config)?;
    validate_management_port_distinct_from_server(config)?;
    Ok(())
}

/// A separate management plane must not bind the data-plane port.
///
/// A cross-section invariant no single config owner can check, because it spans
/// `management` and `server`. A wildcard listener host overlaps every host, so
/// two planes sharing a port always collide regardless of the hosts each is
/// configured with; the ports alone are compared.
fn validate_management_port_distinct_from_server(config: &RuntimeConfig) -> Result<(), ConfigurationError> {
    let management = &config.management;
    if matches!(management.mode, ManagementMode::SeparatePort) && management.port == config.server.port {
        return Err(owner_configuration_error(ManagementConfigError::PortCollision { port: management.port }));
    }
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
    let acquire = config.database.acquire_timeout.duration().as_secs();
    let request = config.http.request_timeout.duration().as_secs();
    if acquire >= request {
        return Err(ConfigurationError::invalid(format!(
            "database.acquire_timeout ({acquire}s) must be below http.request_timeout ({request}s) so a saturated pool answers 503 before the request deadline"
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
    use crate::config::{
        management::ManagementMode,
        runtime::RuntimeConfig,
        validation::{validate_acquire_timeout_below_request_timeout, validate_management_port_distinct_from_server},
    };
    use andromeda_configuration_scalars::human_duration::HumanDuration;

    #[test]
    fn management_port_colliding_with_server_port_is_rejected() {
        let mut config = RuntimeConfig::default();
        config.management.mode = ManagementMode::SeparatePort;

        config.management.port = config.server.port + 1;
        assert!(
            validate_management_port_distinct_from_server(&config).is_ok(),
            "a separate management port distinct from the data-plane port is accepted"
        );

        config.management.port = config.server.port;
        assert!(
            validate_management_port_distinct_from_server(&config).is_err(),
            "a separate management port equal to the data-plane port is rejected"
        );

        // In merged mode the management endpoints ride the data-plane listener,
        // so the two ports matching is expected rather than a collision.
        config.management.mode = ManagementMode::Merged;
        assert!(
            validate_management_port_distinct_from_server(&config).is_ok(),
            "a merged plane sharing the data-plane port is accepted"
        );
    }

    #[test]
    fn acquire_timeout_must_be_below_the_request_timeout() {
        let mut config = RuntimeConfig::default();
        config.http.request_timeout = HumanDuration::from_secs(10);

        config.database.acquire_timeout = HumanDuration::from_secs(9);
        assert!(
            validate_acquire_timeout_below_request_timeout(&config).is_ok(),
            "an acquire below the request timeout is accepted"
        );

        config.database.acquire_timeout = HumanDuration::from_secs(10);
        assert!(
            validate_acquire_timeout_below_request_timeout(&config).is_err(),
            "an acquire equal to the request timeout is rejected"
        );

        config.database.acquire_timeout = HumanDuration::from_secs(11);
        assert!(
            validate_acquire_timeout_below_request_timeout(&config).is_err(),
            "an acquire above the request timeout is rejected"
        );
    }
}
