//! The rules that span more than one table, which no one table can check.

use crate::{
    broker_config::BrokerConfig,
    schema::{
        http3::Http3Mode,
        listener_transport::ListenerTransport,
        management::ManagementMode,
        observability::MetricsMode,
        tls::{ClientAuthenticationMode, TlsConfig},
    },
    validate::{
        bound::Relation,
        config_key::{ConfigKey, QueueKey, QueueName},
        rule::{CompoundRule, Limit, Rule},
        violation::{ConfigurationViolation, Requirement},
        violation_log::ViolationLog,
    },
};
use std::time::Duration;

/// How much longer than the connection acquire deadline a side-effect lease
/// must run.
///
/// A job's lease has to last from its claim to the first statement of its
/// replay, which removes the job and locks it for the rest of the replay. That
/// wait is chiefly acquiring the replay transaction's connection, which the
/// acquire deadline bounds; the margin covers the rest of it, decoding the
/// claimed job, matching the notifications it still owes, and the settling
/// statement itself, before another worker could reclaim the job.
const SIDE_EFFECT_LEASE_MARGIN: Duration = Duration::from_secs(5);

/// A saturated pool and a tenant cold build each answer 503 with
/// `Retry-After` before the request deadline races them, and a side-effect
/// lease outlasts the wait for its replay's connection. A provisional tenant
/// outlives the longest creating request, or a request still running could
/// find its database gone.
pub(crate) fn rules() -> impl Iterator<Item = Rule> {
    [
        Rule::Ordered {
            key: ConfigKey::DatabaseAcquireTimeout,
            relation: Relation::Below,
            limit: Limit::Key(ConfigKey::RequestLimitsTimeout),
        },
        Rule::Ordered {
            key: ConfigKey::Queue(QueueName::EntitySideEffects, QueueKey::LeaseTimeout),
            relation: Relation::AtLeast,
            limit: Limit::KeyPlusMargin(ConfigKey::DatabaseAcquireTimeout, SIDE_EFFECT_LEASE_MARGIN),
        },
        Rule::Ordered {
            key: ConfigKey::TenancyBuildAdmissionTimeout,
            relation: Relation::Below,
            limit: Limit::Key(ConfigKey::RequestLimitsTimeout),
        },
        Rule::Ordered {
            key: ConfigKey::TenancyProvisionalTenantLifetime,
            relation: Relation::Above,
            limit: Limit::Key(ConfigKey::RequestLimitsTimeout),
        },
    ]
    .into_iter()
}

/// What [`check_listeners`] asks.
pub(crate) const COMPOUND_RULES: [CompoundRule; 4] = [
    CompoundRule {
        keys: &[ConfigKey::ManagementPort, ConfigKey::ServerPort],
        statement: "With `management.mode` at `\"separate-port\"`, `management.port` differs from `server.port`.",
    },
    CompoundRule {
        keys: &[ConfigKey::ObservabilityMetricsMode, ConfigKey::ManagementMode],
        statement: "Metrics are served on the management plane, so `observability.metrics.mode` at `\"prometheus\"` needs `management.mode` other than `\"disabled\"`.",
    },
    CompoundRule {
        keys: &[ConfigKey::TlsCertificate, ConfigKey::TlsPrivateKey],
        statement: "`tls.certificate` and `tls.private_key` are required as soon as a listener terminates TLS: `server.transport` at `\"tls\"`, `server.http3.mode` at `\"enabled\"`, or `management.transport` at `\"tls\"` with `management.mode` at `\"separate-port\"`.",
    },
    CompoundRule {
        keys: &[ConfigKey::TlsClientAuthenticationCa, ConfigKey::TlsClientAuthenticationMode],
        statement: "Once a listener terminates TLS, `tls.client_authentication.ca` is required while `tls.client_authentication.mode` is `\"optional\"` or `\"required\"`, and refused while it is `\"disabled\"`.",
    },
];

/// A separate management plane binds its own port, metrics have a management
/// plane to be served on, and every listener that terminates TLS has an
/// identity to present.
pub(crate) fn check_listeners(config: &BrokerConfig, log: &mut ViolationLog) {
    let management = &config.management;
    if matches!(management.mode, ManagementMode::SeparatePort) && management.port == config.server.port {
        log.record(ConfigurationViolation::ManagementPortCollision { port: management.port });
    }
    match (config.observability.metrics.mode, management.mode) {
        (MetricsMode::Prometheus, ManagementMode::Disabled) => log.record(ConfigurationViolation::MetricsWithoutManagementPlane),
        (MetricsMode::Prometheus | MetricsMode::Disabled, ManagementMode::SeparatePort | ManagementMode::SharedPort | ManagementMode::Disabled) => {}
    }
    if let Some(listener) = first_tls_listener(config) {
        check_tls(&config.tls, listener, log);
    }
}

/// The first listener that terminates TLS, as the setting that makes it do
/// so; `None` when none does.
///
/// The `[tls]` table is inert until a listener asks for it. QUIC is always
/// TLS, so an enabled HTTP/3 listener always asks; the management listener
/// asks only in `separate-port` mode, the only mode in which its `transport`
/// is read.
fn first_tls_listener(config: &BrokerConfig) -> Option<Requirement> {
    let server = matches!(config.server.transport, ListenerTransport::Tls).then_some(Requirement::ServerTls);
    let http3 = matches!(config.server.http3.mode, Http3Mode::Enabled).then_some(Requirement::Http3);
    let management = matches!(
        (config.management.mode, config.management.transport),
        (ManagementMode::SeparatePort, ListenerTransport::Tls)
    )
    .then_some(Requirement::ManagementTls);
    server.or(http3).or(management)
}

/// The identity a TLS listener presents, and the client CA exactly when client
/// certificates are asked for.
fn check_tls(tls: &TlsConfig, listener: Requirement, log: &mut ViolationLog) {
    let unset = |key| ConfigurationViolation::Unset { key, required_by: listener };
    if tls.certificate.is_none() {
        log.record(unset(ConfigKey::TlsCertificate));
    }
    if tls.private_key.is_none() {
        log.record(unset(ConfigKey::TlsPrivateKey));
    }
    match (tls.client_authentication.mode, &tls.client_authentication.ca) {
        (mode @ (ClientAuthenticationMode::Optional | ClientAuthenticationMode::Required), None) => log.record(ConfigurationViolation::Unset {
            key: ConfigKey::TlsClientAuthenticationCa,
            required_by: Requirement::ClientAuthentication(mode),
        }),
        (ClientAuthenticationMode::Disabled, Some(_)) => log.record(ConfigurationViolation::ClientCaWithoutClientAuthentication),
        (ClientAuthenticationMode::Disabled, None) | (ClientAuthenticationMode::Optional | ClientAuthenticationMode::Required, Some(_)) => {}
    }
}

#[cfg(test)]
mod tests;
