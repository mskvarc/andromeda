//! The rules that span more than one table, which no one table can check.

use crate::{
    schema::{feature_mode::FeatureMode, management::ManagementMode, tls::ClientAuthenticationMode},
    validate::{
        bound::Relation,
        choice_value::ChoiceValue,
        compound_rule::{CompoundRule, Condition, Obligation},
        config_key::{ConfigKey, QueueKey, QueueName},
        rule::{Limit, Rule},
    },
};
use std::time::Duration;

/// How much longer than the connection acquire deadline a side-effect or a
/// history lease must run.
///
/// A side-effect job's lease has to last from its claim to the first
/// statement of its replay, which removes the job and locks it for the rest of
/// the replay; a history batch's lease has to last from its claim to the
/// statement that removes the applied intents under it. Either wait is chiefly
/// acquiring the transaction's connection, which the acquire deadline bounds;
/// the margin covers the rest of it, decoding the claimed work and the
/// statements before the removal, before another worker could reclaim it.
const REPLAY_LEASE_MARGIN: Duration = Duration::from_secs(5);

/// A saturated pool, a tenant cold build and a Snapshot still being prepared
/// each answer 503 with `Retry-After`, and a `@context` that cannot be
/// downloaded answers 504 `LdContextNotAvailable`, only if their waits end
/// before the request deadline answers 500 in their place; a forwarded request
/// likewise counts as a failed source, answered as the operation answers one,
/// only if it gives up before the request does. An HTTP
/// notification carries in full the entities a change wrote, so its body limit
/// exceeds the largest body a request may send, and so does the MQTT packet a
/// notification is published in. The storage a tenant's posted queries may
/// take holds at least the largest body a request may send, so a query a
/// client may post fits on its own. A side-effect or history
/// lease outlasts the wait for its transaction's connection. A provisional
/// tenant outlives the longest creating request, or a request still running
/// could find its database gone. A tenant pool behind a transaction-pooling
/// proxy holds only the connections its concurrent requests need, the proxy
/// multiplexing them onto its backends, so it is never larger than the pool
/// the tenant opens straight to PostgreSQL.
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
            limit: Limit::KeyPlusMargin(ConfigKey::DatabaseAcquireTimeout, REPLAY_LEASE_MARGIN),
        },
        Rule::Ordered {
            key: ConfigKey::Queue(QueueName::TemporalHistory, QueueKey::LeaseTimeout),
            relation: Relation::AtLeast,
            limit: Limit::KeyPlusMargin(ConfigKey::DatabaseAcquireTimeout, REPLAY_LEASE_MARGIN),
        },
        Rule::Ordered {
            key: ConfigKey::TenancyBuildAdmissionTimeout,
            relation: Relation::Below,
            limit: Limit::Key(ConfigKey::RequestLimitsTimeout),
        },
        Rule::Ordered {
            key: ConfigKey::SnapshotPreparationScopedOperationTimeout,
            relation: Relation::Below,
            limit: Limit::Key(ConfigKey::RequestLimitsTimeout),
        },
        Rule::Ordered {
            key: ConfigKey::JsonLdFetchTimeout,
            relation: Relation::Below,
            limit: Limit::Key(ConfigKey::RequestLimitsTimeout),
        },
        Rule::Ordered {
            key: ConfigKey::FederationForwardingRequestTimeout,
            relation: Relation::Below,
            limit: Limit::Key(ConfigKey::RequestLimitsTimeout),
        },
        Rule::Ordered {
            key: ConfigKey::NotificationsDeliveryMaxHttpBody,
            relation: Relation::Above,
            limit: Limit::Key(ConfigKey::RequestLimitsMaxBody),
        },
        Rule::Ordered {
            key: ConfigKey::NotificationsDeliveryMaxMqttPacket,
            relation: Relation::Above,
            limit: Limit::Key(ConfigKey::RequestLimitsMaxBody),
        },
        Rule::Ordered {
            key: ConfigKey::PostedQueryMaxStoragePerTenant,
            relation: Relation::AtLeast,
            limit: Limit::Key(ConfigKey::RequestLimitsMaxBody),
        },
        Rule::Ordered {
            key: ConfigKey::DatabasePoolerMaxConnectionsPerTenant,
            relation: Relation::AtMost,
            limit: Limit::Key(ConfigKey::TenancyDatabasesMaxConnections),
        },
        Rule::Ordered {
            key: ConfigKey::TenancyProvisionalTenantLifetime,
            relation: Relation::Above,
            limit: Limit::Key(ConfigKey::RequestLimitsTimeout),
        },
    ]
    .into_iter()
}

/// A separate management plane binds its own port, metrics have a management
/// plane to be served on, and every listener that terminates TLS has an
/// identity to present and reads a client CA exactly when client certificates
/// are asked for. The `[tls]` table is inert until a listener asks for it.
pub(crate) const COMPOUND_RULES: [CompoundRule; 5] = [
    CompoundRule {
        statement: "With `management.mode` at `\"separate-port\"`, `management.port` differs from `server.port`.",
        when: &[Condition::Is {
            key: ConfigKey::ManagementMode,
            choices: &[ChoiceValue::Management(ManagementMode::SeparatePort)],
        }],
        then: &[Obligation::Differs {
            key: ConfigKey::ManagementPort,
            other: ConfigKey::ServerPort,
        }],
    },
    CompoundRule {
        statement: "Metrics are served on the management plane, so `observability.metrics.prometheus.mode` at `\"enabled\"` needs `management.mode` other than `\"disabled\"`.",
        when: &[Condition::Is {
            key: ConfigKey::ObservabilityMetricsPrometheusMode,
            choices: &[ChoiceValue::Feature(FeatureMode::Enabled)],
        }],
        then: &[Obligation::IsNot {
            key: ConfigKey::ManagementMode,
            choice: ChoiceValue::Management(ManagementMode::Disabled),
        }],
    },
    CompoundRule {
        statement: "`tls.certificate` and `tls.private_key` are required as soon as a listener terminates TLS: `server.transport` at `\"tls\"`, `server.http3.mode` at `\"enabled\"`, or `management.transport` at `\"tls\"` with `management.mode` at `\"separate-port\"`.",
        when: &[Condition::TlsListener],
        then: &[Obligation::Present(ConfigKey::TlsCertificate), Obligation::Present(ConfigKey::TlsPrivateKey)],
    },
    CompoundRule {
        statement: "Once a listener terminates TLS, `tls.client_authentication.ca` is required while `tls.client_authentication.mode` is `\"optional\"` or `\"required\"`.",
        when: &[
            Condition::TlsListener,
            Condition::Is {
                key: ConfigKey::TlsClientAuthenticationMode,
                choices: &[
                    ChoiceValue::ClientAuthentication(ClientAuthenticationMode::Optional),
                    ChoiceValue::ClientAuthentication(ClientAuthenticationMode::Required),
                ],
            },
        ],
        then: &[Obligation::Present(ConfigKey::TlsClientAuthenticationCa)],
    },
    CompoundRule {
        statement: "Once a listener terminates TLS, `tls.client_authentication.ca` is refused while `tls.client_authentication.mode` is `\"disabled\"`.",
        when: &[
            Condition::TlsListener,
            Condition::Is {
                key: ConfigKey::TlsClientAuthenticationMode,
                choices: &[ChoiceValue::ClientAuthentication(ClientAuthenticationMode::Disabled)],
            },
        ],
        then: &[Obligation::Absent(ConfigKey::TlsClientAuthenticationCa)],
    },
];

#[cfg(test)]
mod tests;
