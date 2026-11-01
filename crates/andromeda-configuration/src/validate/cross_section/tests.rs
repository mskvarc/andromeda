use crate::{
    broker_config::BrokerConfig,
    human_byte_size::HumanByteSize,
    human_duration::HumanDuration,
    mqtt_packet_limit::{MqttPacketLimit, MqttPacketLimitError},
    notification_body_limit::{NotificationBodyLimit, NotificationBodyLimitError},
    schema::{feature_mode::FeatureMode, http3::Http3Mode, listener_transport::ListenerTransport, management::ManagementMode, tls::ClientAuthenticationMode},
    validate::{
        bound::{Bound, Quantity, Relation},
        config_key::{ConfigKey, QueueKey, QueueName},
        rule_tests::{assert_accepted, refusals},
        violation::{ConfigurationViolation, Requirement},
    },
};
use pretty_assertions::assert_eq;
use std::{path::PathBuf, time::Duration};

/// The refusal of a duration `key` holding `value` against the request
/// timeout `request`.
fn against_request_timeout(key: ConfigKey, value: Duration, relation: Relation, request: Duration) -> ConfigurationViolation {
    ConfigurationViolation::OutOfRange {
        key,
        value: Quantity::Duration(value),
        relation,
        bound: Bound::setting(ConfigKey::RequestLimitsTimeout, request),
    }
}

/// Both key-pair files, and nothing else of the `[tls]` table.
fn with_key_pair(config: &mut BrokerConfig) {
    config.tls.certificate = Some(PathBuf::from("/etc/andromeda/broker.crt"));
    config.tls.private_key = Some(PathBuf::from("/etc/andromeda/broker.key"));
}

/// The refusals of a TLS listener asked for by `listener` with no key pair.
fn missing_key_pair(listener: Requirement) -> [ConfigurationViolation; 2] {
    [
        ConfigurationViolation::Unset {
            key: ConfigKey::TlsCertificate,
            required_by: listener,
        },
        ConfigurationViolation::Unset {
            key: ConfigKey::TlsPrivateKey,
            required_by: listener,
        },
    ]
}

/// The side-effect and history leases are lengthened so only the acquire rule
/// is at its boundary.
#[test]
fn an_acquire_timeout_at_the_request_timeout_is_refused_and_one_step_below_accepted() {
    let mut config = BrokerConfig::default();
    config.entity.side_effects.queue.lease_timeout = HumanDuration::from_secs(35);
    config.temporal.history.queue.lease_timeout = HumanDuration::from_secs(35);
    config.database.acquire_timeout = HumanDuration::from_secs(30);
    assert_eq!(
        refusals(&config),
        [against_request_timeout(
            ConfigKey::DatabaseAcquireTimeout,
            Duration::from_secs(30),
            Relation::Below,
            Duration::from_secs(30)
        )]
    );
    config.database.acquire_timeout = HumanDuration::from_millis(29_999);
    assert_accepted(&config);
}

#[test]
fn a_side_effect_lease_short_of_its_margin_over_the_acquire_timeout_is_refused_and_the_margin_exactly_accepted() {
    let mut config = BrokerConfig::default();
    config.database.acquire_timeout = HumanDuration::from_secs(5);
    config.entity.side_effects.queue.lease_timeout = HumanDuration::from_millis(9_999);
    assert_eq!(
        refusals(&config),
        [ConfigurationViolation::OutOfRange {
            key: ConfigKey::Queue(QueueName::EntitySideEffects, QueueKey::LeaseTimeout),
            value: Quantity::Duration(Duration::from_millis(9_999)),
            relation: Relation::AtLeast,
            bound: Bound::SettingPlusMargin {
                key: ConfigKey::DatabaseAcquireTimeout,
                value: Duration::from_secs(5),
                margin: Duration::from_secs(5),
            },
        }]
    );
    config.entity.side_effects.queue.lease_timeout = HumanDuration::from_secs(10);
    assert_accepted(&config);
}

#[test]
fn a_history_lease_short_of_its_margin_over_the_acquire_timeout_is_refused_and_the_margin_exactly_accepted() {
    let mut config = BrokerConfig::default();
    config.database.acquire_timeout = HumanDuration::from_secs(5);
    config.temporal.history.queue.lease_timeout = HumanDuration::from_millis(9_999);
    assert_eq!(
        refusals(&config),
        [ConfigurationViolation::OutOfRange {
            key: ConfigKey::Queue(QueueName::TemporalHistory, QueueKey::LeaseTimeout),
            value: Quantity::Duration(Duration::from_millis(9_999)),
            relation: Relation::AtLeast,
            bound: Bound::SettingPlusMargin {
                key: ConfigKey::DatabaseAcquireTimeout,
                value: Duration::from_secs(5),
                margin: Duration::from_secs(5),
            },
        }]
    );
    config.temporal.history.queue.lease_timeout = HumanDuration::from_secs(10);
    assert_accepted(&config);
}

#[test]
fn a_build_admission_timeout_at_the_request_timeout_is_refused_and_one_step_below_accepted() {
    let mut config = BrokerConfig::default();
    config.tenancy.build.admission_timeout = HumanDuration::from_secs(30);
    assert_eq!(
        refusals(&config),
        [against_request_timeout(
            ConfigKey::TenancyBuildAdmissionTimeout,
            Duration::from_secs(30),
            Relation::Below,
            Duration::from_secs(30)
        )]
    );
    config.tenancy.build.admission_timeout = HumanDuration::from_millis(29_999);
    assert_accepted(&config);
}

#[test]
fn a_scoped_operation_timeout_at_the_request_timeout_is_refused_and_one_step_below_accepted() {
    let mut config = BrokerConfig::default();
    config.snapshot.preparation.scoped_operation_timeout = HumanDuration::from_secs(30);
    assert_eq!(
        refusals(&config),
        [against_request_timeout(
            ConfigKey::SnapshotPreparationScopedOperationTimeout,
            Duration::from_secs(30),
            Relation::Below,
            Duration::from_secs(30)
        )]
    );
    config.snapshot.preparation.scoped_operation_timeout = HumanDuration::from_millis(29_999);
    assert_accepted(&config);
}

#[test]
fn a_context_fetch_timeout_at_the_request_timeout_is_refused_and_one_step_below_accepted() {
    let mut config = BrokerConfig::default();
    config.jsonld.fetch.timeout = HumanDuration::from_secs(30);
    assert_eq!(
        refusals(&config),
        [against_request_timeout(
            ConfigKey::JsonLdFetchTimeout,
            Duration::from_secs(30),
            Relation::Below,
            Duration::from_secs(30)
        )]
    );
    config.jsonld.fetch.timeout = HumanDuration::from_millis(29_999);
    assert_accepted(&config);
}

#[test]
fn a_notification_body_limit_at_the_request_body_limit_is_refused_and_one_byte_above_accepted() -> Result<(), NotificationBodyLimitError> {
    let mut config = BrokerConfig::default();
    config.request_limits.max_body = HumanByteSize::mib(64);
    config.notifications.delivery.max_http_body = NotificationBodyLimit::try_from(HumanByteSize::mib(64))?;
    assert_eq!(
        refusals(&config),
        [ConfigurationViolation::OutOfRange {
            key: ConfigKey::NotificationsDeliveryMaxHttpBody,
            value: Quantity::Bytes(67_108_864),
            relation: Relation::Above,
            bound: Bound::setting(ConfigKey::RequestLimitsMaxBody, Quantity::Bytes(67_108_864)),
        }]
    );
    config.notifications.delivery.max_http_body = NotificationBodyLimit::try_from(HumanByteSize::b(67_108_865))?;
    assert_accepted(&config);
    Ok(())
}

#[test]
fn an_mqtt_packet_limit_at_the_request_body_limit_is_refused_and_one_byte_above_accepted() -> Result<(), MqttPacketLimitError> {
    let mut config = BrokerConfig::default();
    config.request_limits.max_body = HumanByteSize::mib(64);
    config.notifications.delivery.max_mqtt_packet = MqttPacketLimit::try_from(HumanByteSize::mib(64))?;
    assert_eq!(
        refusals(&config),
        [ConfigurationViolation::OutOfRange {
            key: ConfigKey::NotificationsDeliveryMaxMqttPacket,
            value: Quantity::Bytes(67_108_864),
            relation: Relation::Above,
            bound: Bound::setting(ConfigKey::RequestLimitsMaxBody, Quantity::Bytes(67_108_864)),
        }]
    );
    config.notifications.delivery.max_mqtt_packet = MqttPacketLimit::try_from(HumanByteSize::b(67_108_865))?;
    assert_accepted(&config);
    Ok(())
}

#[test]
fn a_provisional_tenant_lifetime_at_the_request_timeout_is_refused_and_one_step_above_accepted() {
    let mut config = BrokerConfig::default();
    config.tenancy.provisional_tenant_lifetime = HumanDuration::from_secs(30);
    assert_eq!(
        refusals(&config),
        [against_request_timeout(
            ConfigKey::TenancyProvisionalTenantLifetime,
            Duration::from_secs(30),
            Relation::Above,
            Duration::from_secs(30)
        )]
    );
    config.tenancy.provisional_tenant_lifetime = HumanDuration::from_millis(30_001);
    assert_accepted(&config);
}

#[test]
fn a_separate_management_port_equal_to_the_server_port_is_refused_and_the_next_port_accepted() {
    let mut config = BrokerConfig::default();
    config.management.mode = ManagementMode::SeparatePort;
    config.management.port = config.server.port;
    assert_eq!(
        refusals(&config),
        [ConfigurationViolation::ManagementPortCollision { port: config.server.port }]
    );
    config.management.port = config.server.port + 1;
    assert_accepted(&config);
}

/// The management port is not read while the endpoints ride the data plane.
#[test]
fn a_shared_port_management_plane_never_collides() {
    let mut config = BrokerConfig::default();
    config.management.mode = ManagementMode::SharedPort;
    config.management.port = config.server.port;
    assert_accepted(&config);
}

#[test]
fn metrics_without_a_management_plane_are_refused_and_on_a_shared_port_accepted() {
    let mut config = BrokerConfig::default();
    config.observability.metrics.prometheus.mode = FeatureMode::Enabled;
    config.management.mode = ManagementMode::Disabled;
    assert_eq!(refusals(&config), [ConfigurationViolation::MetricsWithoutManagementPlane]);
    config.management.mode = ManagementMode::SharedPort;
    assert_accepted(&config);
}

#[test]
fn a_tls_listener_without_a_key_pair_is_refused_for_both_files_and_with_one_accepted() {
    let mut config = BrokerConfig::default();
    config.server.transport = ListenerTransport::Tls;
    assert_eq!(refusals(&config), missing_key_pair(Requirement::ServerTls));
    with_key_pair(&mut config);
    assert_accepted(&config);
}

#[test]
fn an_enabled_http3_listener_asks_for_the_key_pair() {
    let mut config = BrokerConfig::default();
    config.server.http3.mode = Http3Mode::Enabled;
    assert_eq!(refusals(&config), missing_key_pair(Requirement::Http3));
}

#[test]
fn a_separate_port_tls_management_listener_asks_for_the_key_pair() {
    let mut config = BrokerConfig::default();
    config.management.mode = ManagementMode::SeparatePort;
    config.management.transport = ListenerTransport::Tls;
    assert_eq!(refusals(&config), missing_key_pair(Requirement::ManagementTls));
}

/// The management `transport` is read only in `separate-port` mode, so it
/// asks for no identity when the management endpoints ride the data plane.
#[test]
fn a_shared_port_tls_management_transport_needs_no_key_pair() {
    let mut config = BrokerConfig::default();
    config.management.mode = ManagementMode::SharedPort;
    config.management.transport = ListenerTransport::Tls;
    assert_accepted(&config);
}

#[test]
fn client_authentication_without_a_ca_is_refused_and_with_one_accepted() {
    for mode in [ClientAuthenticationMode::Optional, ClientAuthenticationMode::Required] {
        let mut config = BrokerConfig::default();
        config.server.transport = ListenerTransport::Tls;
        with_key_pair(&mut config);
        config.tls.client_authentication.mode = mode;
        assert_eq!(
            refusals(&config),
            [ConfigurationViolation::Unset {
                key: ConfigKey::TlsClientAuthenticationCa,
                required_by: Requirement::ClientAuthentication(mode),
            }]
        );
        config.tls.client_authentication.ca = Some(PathBuf::from("/etc/andromeda/clients.pem"));
        assert_accepted(&config);
    }
}

#[test]
fn a_ca_without_client_authentication_is_refused() {
    let mut config = BrokerConfig::default();
    config.server.transport = ListenerTransport::Tls;
    with_key_pair(&mut config);
    config.tls.client_authentication.ca = Some(PathBuf::from("/etc/andromeda/clients.pem"));
    assert_eq!(refusals(&config), [ConfigurationViolation::ClientCaWithoutClientAuthentication]);
}

/// The `[tls]` table is inert until a listener terminates TLS.
#[test]
fn an_incomplete_tls_table_is_not_read_without_a_tls_listener() {
    let mut config = BrokerConfig::default();
    config.tls.client_authentication.mode = ClientAuthenticationMode::Required;
    assert_accepted(&config);
}
