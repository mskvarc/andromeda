//! The rules that span more than one table, which no one table can check.

use crate::{
    broker_config::BrokerConfig,
    schema::{
        http3::Http3Mode,
        listener_transport::ListenerTransport,
        management::{ManagementConfigError, ManagementMode},
        observability::MetricsMode,
    },
    validate::InvalidConfiguration,
};

/// Every listener that terminates TLS must have an identity to present.
///
/// The `[tls]` table is inert until a listener asks for it, so it is
/// validated only then. QUIC is always TLS, so an enabled HTTP/3 listener
/// always asks; the management listener asks only in `separate-port` mode,
/// the only mode in which its `transport` is read.
pub(crate) fn check_tls_material_for_listeners(config: &BrokerConfig) -> Result<(), InvalidConfiguration> {
    let tcp = matches!(config.server.transport, ListenerTransport::Tls);
    let quic = matches!(config.server.http3.mode, Http3Mode::Enabled);
    let management = matches!(
        (config.management.mode, config.management.transport),
        (ManagementMode::SeparatePort, ListenerTransport::Tls)
    );
    if tcp || quic || management {
        config.tls.material_sources()?;
    }
    Ok(())
}

/// A separate management plane must not bind the data-plane port.
///
/// A wildcard listener host overlaps every host, so two planes sharing a
/// port always collide whatever hosts they name; the ports alone are
/// compared.
pub(crate) fn check_management_port_distinct_from_server(config: &BrokerConfig) -> Result<(), InvalidConfiguration> {
    let management = &config.management;
    if matches!(management.mode, ManagementMode::SeparatePort) && management.port == config.server.port {
        return Err(ManagementConfigError::PortCollision { port: management.port }.into());
    }
    Ok(())
}

/// Metrics are served on the management plane, so one must exist.
pub(crate) fn check_metrics_have_a_management_plane(config: &BrokerConfig) -> Result<(), InvalidConfiguration> {
    match (config.observability.metrics.mode, config.management.mode) {
        (MetricsMode::Prometheus, ManagementMode::Disabled) => Err(ManagementConfigError::MetricsWithoutManagementPlane.into()),
        (MetricsMode::Prometheus | MetricsMode::Disabled, ManagementMode::SeparatePort | ManagementMode::SharedPort | ManagementMode::Disabled) => Ok(()),
    }
}

/// The connection acquire deadline must be strictly below the request
/// timeout.
///
/// If the pool waited as long as the request deadline for a connection, a
/// saturated pool would race the request timeout and surface as a stalled 500
/// rather than the fast 503 with `Retry-After` the acquire deadline exists to
/// produce.
pub(crate) const fn check_acquire_timeout_below_request_timeout(config: &BrokerConfig) -> Result<(), InvalidConfiguration> {
    let acquire = config.database.acquire_timeout.duration();
    let request = config.request_limits.timeout.duration();
    if acquire.as_nanos() >= request.as_nanos() {
        return Err(InvalidConfiguration::AcquireTimeoutNotBelowRequestTimeout { acquire, request });
    }
    Ok(())
}

/// The side-effect lease must outlast the connection acquire deadline by the
/// entity table's margin.
///
/// A job's lease has to last until its replay's first statement locks it, and
/// acquiring that replay's connection may take up to `database.acquire_timeout`.
pub(crate) fn check_side_effect_lease_over_acquire_timeout(config: &BrokerConfig) -> Result<(), InvalidConfiguration> {
    config
        .entity
        .side_effects
        .validate_lease_over(config.database.acquire_timeout.duration())
        .map_err(|error| InvalidConfiguration::Entity(error.into()))
}

/// The tenant cold-build admission deadline must be strictly below the
/// request timeout.
///
/// The admission deadline exists so a request that cannot be admitted to a
/// tenant's cold build is shed as a 503 with `Retry-After`; at or past the
/// request deadline it would race the request timeout and never get the
/// chance.
pub(crate) fn check_build_admission_below_request_timeout(config: &BrokerConfig) -> Result<(), InvalidConfiguration> {
    let admission = config.tenancy.build.admission_timeout.duration();
    let request = config.request_limits.timeout.duration();
    if admission >= request {
        return Err(InvalidConfiguration::BuildAdmissionNotBelowRequestTimeout { admission, request });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::{
        broker_config::BrokerConfig,
        human_duration::HumanDuration,
        schema::{http3::Http3Mode, listener_transport::ListenerTransport, management::ManagementMode, observability::MetricsMode},
        validate::cross_section::{
            check_acquire_timeout_below_request_timeout,
            check_build_admission_below_request_timeout,
            check_management_port_distinct_from_server,
            check_metrics_have_a_management_plane,
            check_side_effect_lease_over_acquire_timeout,
            check_tls_material_for_listeners,
        },
    };
    use std::path::PathBuf;

    /// Whether the TLS cross-section check refuses `config` for want of a
    /// certificate.
    fn refused_for_missing_certificate(config: &BrokerConfig) -> bool {
        check_tls_material_for_listeners(config)
            .map_err(|error| error.to_string())
            .is_err_and(|message| message.contains("tls.certificate must be set"))
    }

    #[test]
    fn tls_transport_without_certificate_is_rejected() {
        let mut config = BrokerConfig::default();
        config.server.transport = ListenerTransport::Tls;
        assert!(refused_for_missing_certificate(&config));
    }

    #[test]
    fn http3_enabled_without_tls_material_is_rejected() {
        let mut config = BrokerConfig::default();
        config.server.http3.mode = Http3Mode::Enabled;
        assert!(refused_for_missing_certificate(&config));
    }

    #[test]
    fn separate_port_management_tls_without_material_is_rejected() {
        let mut config = BrokerConfig::default();
        config.management.mode = ManagementMode::SeparatePort;
        config.management.transport = ListenerTransport::Tls;
        assert!(refused_for_missing_certificate(&config));
    }

    /// The management `transport` is read only in `separate-port` mode, so it
    /// asks for no identity when the management endpoints ride the data plane.
    #[test]
    fn shared_port_management_tls_transport_needs_no_material() {
        let mut config = BrokerConfig::default();
        config.management.mode = ManagementMode::SharedPort;
        config.management.transport = ListenerTransport::Tls;
        assert!(check_tls_material_for_listeners(&config).is_ok());
    }

    #[test]
    fn the_default_configuration_needs_no_tls_material() {
        assert!(check_tls_material_for_listeners(&BrokerConfig::default()).is_ok());
    }

    #[test]
    fn a_tls_listener_with_a_key_pair_is_accepted() {
        let mut config = BrokerConfig::default();
        config.server.transport = ListenerTransport::Tls;
        config.tls.certificate = Some(PathBuf::from("/etc/andromeda/broker.crt"));
        config.tls.private_key = Some(PathBuf::from("/etc/andromeda/broker.key"));
        assert!(check_tls_material_for_listeners(&config).is_ok());
    }

    #[test]
    fn management_port_colliding_with_server_port_is_rejected() {
        let mut config = BrokerConfig::default();
        config.management.mode = ManagementMode::SeparatePort;
        config.management.port = config.server.port + 1;
        assert!(check_management_port_distinct_from_server(&config).is_ok());
        config.management.port = config.server.port;
        assert!(check_management_port_distinct_from_server(&config).is_err());
        config.management.mode = ManagementMode::SharedPort;
        assert!(check_management_port_distinct_from_server(&config).is_ok());
    }

    #[test]
    fn metrics_without_a_management_plane_are_rejected() {
        let mut config = BrokerConfig::default();
        config.observability.metrics.mode = MetricsMode::Prometheus;
        assert!(check_metrics_have_a_management_plane(&config).is_ok());
        config.management.mode = ManagementMode::SharedPort;
        assert!(check_metrics_have_a_management_plane(&config).is_ok());
        config.management.mode = ManagementMode::Disabled;
        assert!(check_metrics_have_a_management_plane(&config).is_err());
    }

    #[test]
    fn acquire_timeout_must_be_below_the_request_timeout() {
        let mut config = BrokerConfig::default();
        config.request_limits.timeout = HumanDuration::from_secs(10);
        config.database.acquire_timeout = HumanDuration::from_millis(9_999);
        assert!(check_acquire_timeout_below_request_timeout(&config).is_ok());
        config.database.acquire_timeout = HumanDuration::from_secs(10);
        assert!(check_acquire_timeout_below_request_timeout(&config).is_err());
    }

    #[test]
    fn build_admission_timeout_must_be_below_the_request_timeout() {
        let mut config = BrokerConfig::default();
        assert!(check_build_admission_below_request_timeout(&config).is_ok());
        config.request_limits.timeout = HumanDuration::from_secs(10);
        config.tenancy.build.admission_timeout = HumanDuration::from_millis(9_999);
        assert!(check_build_admission_below_request_timeout(&config).is_ok());
        config.tenancy.build.admission_timeout = HumanDuration::from_secs(10);
        assert!(check_build_admission_below_request_timeout(&config).is_err());
    }

    /// A side-effect lease that could lapse while its replay waits for a
    /// connection is refused; the shipped defaults pass.
    #[test]
    fn a_side_effect_lease_must_outlast_the_acquire_deadline_by_its_margin() {
        let mut config = BrokerConfig::default();
        assert!(check_side_effect_lease_over_acquire_timeout(&config).is_ok());
        config.database.acquire_timeout = HumanDuration::from_secs(5);
        config.entity.side_effects.queue.lease_timeout = HumanDuration::from_secs(9);
        let refused = check_side_effect_lease_over_acquire_timeout(&config).map_err(|error| error.to_string());
        assert!(
            refused
                .as_ref()
                .is_err_and(|message| message.contains("entity.side_effects.queue.lease_timeout (9s) must exceed database.acquire_timeout (5s)")),
            "{refused:?}"
        );
    }
}
