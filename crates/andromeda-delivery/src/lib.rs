//! Getting a notification from this broker to whoever asked for it.
//!
//! ETSI GS CIM 009 v1.9.1 clause 5.2.15 lets a subscription name an endpoint
//! by URI, and the scheme of that URI decides everything else: `https` is an
//! HTTP POST, `mqtt` is a publish to a pooled broker connection, `sse` and `ws`
//! are a push down a connection the client already opened. [`endpoint`] reads
//! the URI, [`dispatch`] routes on what it read, and each transport module
//! knows only its own protocol.
//!
//! This is a capability, not a policy. Nothing here decides *whether* to
//! notify, *what* the payload says, or *when* to retry — a
//! [`request::DeliveryRequest`] arrives fully formed and the answer is
//! [`dispatch::DeliveryResult`]. The one specification rule the transports do
//! read is clause 5.5.7's: an `application/json` or `application/geo+json`
//! notification carries its `@context` in a `Link` header rather than in the
//! body, and [`request::DeliveryRequest::context_link`] is the single place
//! that decides it.
//!
//! [`mqtt::indicator::MqttHealthIndicator`] points outward, at
//! `andromeda-health`: the connection pool is the only thing that can say
//! whether MQTT delivery is working, so the probe lives here and the word
//! "healthy" lives there.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

pub mod connection_failure;
pub mod delivery_deadline;
pub mod dispatch;
pub mod endpoint;
pub mod error;
pub mod host_port;
pub mod http;
pub mod mqtt;
pub mod notification_context_placement;
pub mod outcome;
pub mod request;
pub mod stream;
pub mod transport;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-health",
        "andromeda-http-headers",
        "andromeda-ngsi-ld-lexicon",
        "andromeda-ngsi-ld-model",
        "andromeda-ngsi-ld-vocabulary",
        "andromeda-telemetry",
    ],
    allow_workspace_dev_dependencies: [
    ],
    allow_production_dependencies: ["bytes", "reqwest", "rumqttc", "tokio"],
}

#[cfg(test)]
andromeda_architecture_checks::forbid_sql!();
