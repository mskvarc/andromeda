//! The rules that span more than one section, which no one section can check.

use crate::{
    broker_config::BrokerConfig,
    schema::{
        http3::Http3Mode,
        listener_transport::ListenerTransport,
        management::{ManagementConfigError, ManagementMode},
    },
    validate::InvalidConfiguration,
};

/// Every listener that terminates TLS must have an identity to present.
///
/// A cross-section invariant spanning `server`, `http3`, `management` and
/// `tls`: the `[tls]` section is inert until one of those asks for it, so it is
/// validated only then. QUIC is always TLS, so an enabled HTTP/3 listener always
/// asks; the management listener asks only in `separate-port` mode, the only
/// mode in which its `transport` is read.
pub(crate) fn check_tls_material_for_listeners(config: &BrokerConfig) -> Result<(), InvalidConfiguration> {
    let tcp = matches!(config.server.transport, ListenerTransport::Tls);
    let quic = matches!(config.http3.mode, Http3Mode::Enabled);
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
/// A cross-section invariant no single config owner can check, because it spans
/// `management` and `server`. A wildcard listener host overlaps every host, so
/// two planes sharing a port always collide regardless of the hosts each is
/// configured with; the ports alone are compared.
pub(crate) fn check_management_port_distinct_from_server(config: &BrokerConfig) -> Result<(), InvalidConfiguration> {
    let management = &config.management;
    if matches!(management.mode, ManagementMode::SeparatePort) && management.port == config.server.port {
        return Err(ManagementConfigError::PortCollision { port: management.port }.into());
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
pub(crate) const fn check_acquire_timeout_below_request_timeout(config: &BrokerConfig) -> Result<(), InvalidConfiguration> {
    let acquire = config.database.acquire_timeout.duration();
    let request = config.http.request_timeout.duration();
    if acquire.as_secs() >= request.as_secs() {
        return Err(InvalidConfiguration::AcquireTimeoutNotBelowRequestTimeout { acquire, request });
    }
    Ok(())
}

/// The side-effect lease must outlast the connection acquire deadline by the
/// entity policy's margin.
///
/// A cross-section invariant spanning `entity` and `database`: a job's lease has
/// to last until its replay's first statement locks it, and acquiring that
/// replay's connection may take up to `database.acquire_timeout`. The entity
/// configuration owns the bound and its margin; this supplies the deadline.
pub(crate) fn check_side_effect_lease_over_acquire_timeout(config: &BrokerConfig) -> Result<(), InvalidConfiguration> {
    Ok(config.entity.validate_side_effect_lease_over(config.database.acquire_timeout.duration())?)
}

/// The tenant cold-build admission deadline must be strictly below the HTTP
/// request timeout.
///
/// A cross-section invariant spanning `tenancy` and `http`. The admission
/// deadline exists so a request that cannot be admitted to a tenant's cold build
/// is shed as a 503 with `Retry-After`; at or past the request deadline it would
/// race the request timeout and never get the chance.
pub(crate) fn check_build_admission_below_request_timeout(config: &BrokerConfig) -> Result<(), InvalidConfiguration> {
    let admission = config.tenancy.build_admission_timeout.duration();
    let request = config.http.request_timeout.duration();
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
        schema::{http3::Http3Mode, listener_transport::ListenerTransport, management::ManagementMode},
        validate::cross_section::{
            check_acquire_timeout_below_request_timeout,
            check_build_admission_below_request_timeout,
            check_management_port_distinct_from_server,
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
            .is_err_and(|message| message.contains("tls.certificate_path must be set"))
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
        config.http3.mode = Http3Mode::Enabled;
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
    fn merged_management_tls_transport_needs_no_material() {
        let mut config = BrokerConfig::default();
        config.management.mode = ManagementMode::Merged;
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
        config.tls.certificate_path = PathBuf::from("/etc/andromeda/broker.crt");
        config.tls.private_key_path = PathBuf::from("/etc/andromeda/broker.key");
        assert!(check_tls_material_for_listeners(&config).is_ok());
    }

    #[test]
    fn management_port_colliding_with_server_port_is_rejected() {
        let mut config = BrokerConfig::default();
        config.management.mode = ManagementMode::SeparatePort;

        config.management.port = config.server.port + 1;
        assert!(
            check_management_port_distinct_from_server(&config).is_ok(),
            "a separate management port distinct from the data-plane port is accepted"
        );

        config.management.port = config.server.port;
        assert!(
            check_management_port_distinct_from_server(&config).is_err(),
            "a separate management port equal to the data-plane port is rejected"
        );

        // In merged mode the management endpoints ride the data-plane listener,
        // so the two ports matching is expected rather than a collision.
        config.management.mode = ManagementMode::Merged;
        assert!(
            check_management_port_distinct_from_server(&config).is_ok(),
            "a merged plane sharing the data-plane port is accepted"
        );
    }

    #[test]
    fn acquire_timeout_must_be_below_the_request_timeout() {
        let mut config = BrokerConfig::default();
        config.http.request_timeout = HumanDuration::from_secs(10);

        config.database.acquire_timeout = HumanDuration::from_secs(9);
        assert!(
            check_acquire_timeout_below_request_timeout(&config).is_ok(),
            "an acquire below the request timeout is accepted"
        );

        config.database.acquire_timeout = HumanDuration::from_secs(10);
        assert!(
            check_acquire_timeout_below_request_timeout(&config).is_err(),
            "an acquire equal to the request timeout is rejected"
        );

        config.database.acquire_timeout = HumanDuration::from_secs(11);
        assert!(
            check_acquire_timeout_below_request_timeout(&config).is_err(),
            "an acquire above the request timeout is rejected"
        );
    }

    #[test]
    fn build_admission_timeout_must_be_below_the_request_timeout() {
        let mut config = BrokerConfig::default();
        assert!(
            check_build_admission_below_request_timeout(&config).is_ok(),
            "the default admission deadline sits below the default request timeout"
        );
        config.http.request_timeout = HumanDuration::from_secs(10);

        config.tenancy.build_admission_timeout = HumanDuration::from_millis(9_999);
        assert!(
            check_build_admission_below_request_timeout(&config).is_ok(),
            "an admission deadline below the request timeout is accepted"
        );

        config.tenancy.build_admission_timeout = HumanDuration::from_secs(10);
        assert!(
            check_build_admission_below_request_timeout(&config).is_err(),
            "an admission deadline equal to the request timeout is rejected"
        );
    }

    /// A side-effect lease that could lapse while its replay waits for a
    /// connection is refused; the shipped defaults pass.
    #[test]
    fn a_side_effect_lease_must_outlast_the_acquire_deadline_by_its_margin() {
        let mut config = BrokerConfig::default();
        assert!(check_side_effect_lease_over_acquire_timeout(&config).is_ok(), "the defaults pass");
        config.database.acquire_timeout = HumanDuration::from_secs(5);
        config.entity.pending_side_effects_lease = HumanDuration::from_secs(9);
        let refused = check_side_effect_lease_over_acquire_timeout(&config).map_err(|error| error.to_string());
        assert!(
            refused
                .as_ref()
                .is_err_and(|message| message.contains("entity.pending_side_effects_lease (9s) must exceed database.acquire_timeout (5s)")),
            "{refused:?}"
        );
    }
}
