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

pub mod dispatch;
pub mod endpoint;
pub mod error;
pub mod http;
pub mod mqtt;
pub mod outcome;
pub mod request;
pub mod stream;
pub mod transport;

#[cfg(test)]
mod boundary {
    /// The layer rule this crate is on the inside of.
    ///
    /// Asserted here rather than by a central test so the failure names the
    /// crate that broke the rule, and so a crate added to the workspace cannot
    /// be forgotten by a table it was never added to. The rule itself is read
    /// once, in `andromeda-layer-guard`, which every crate carries as a
    /// dev-dependency and which declares nothing of its own.
    ///
    /// Delivering a notification is network work, so `reqwest`, `rumqttc` and
    /// the runtime are this crate's to declare. What it must never reach is a
    /// store or the web framework: a transport that could read a subscription
    /// back would be deciding whom to notify, and this crate exists so that
    /// decision stays above it.
    #[test]
    fn this_crate_declares_only_what_its_layer_permits() {
        /// Third-party crates this layer must never ship.
        const FORBIDDEN: &[&str] = &["sqlx", "sea-query", "geozero", "axum", "clap", "confique"];
        /// In-workspace crates this one may name, in any dependency table.
        /// Anything else is an upward or sideways edge.
        const ALLOWED_IN_WORKSPACE: &[&str] = &[
            "andromeda-health",
            "andromeda-layer-guard",
            "andromeda-ngsi-ld-http-headers",
            "andromeda-ngsi-ld-lexicon",
            "andromeda-ngsi-ld-model",
            "andromeda-ngsi-ld-vocabulary",
        ];

        andromeda_layer_guard::manifest::assert_layer(env!("CARGO_MANIFEST_DIR"), FORBIDDEN, ALLOWED_IN_WORKSPACE);
    }
}

#[cfg(test)]
mod dialect {
    /// No storage dialect may be written here, in any form.
    ///
    /// The manifest guard above reads dependency names, which is exactly what a
    /// dialect emitted as a string constant slips past: a crate can spell an
    /// entire SQL predicate without declaring `sea-query`.
    #[test]
    fn no_source_file_in_this_crate_emits_a_storage_dialect() {
        andromeda_layer_guard::dialect::assert_no_dialect(env!("CARGO_MANIFEST_DIR"));
    }
}
