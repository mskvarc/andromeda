//! The distributed side of subscriptions (ETSI GS CIM 009 clauses 5.8.1.4,
//! 5.8.2.4, 5.8.5.4 and 5.8.6).
//!
//! A local subscription is mirrored by a companion context source registration
//! subscription, whose notifications tell this broker which registered sources
//! match; a reduced copy of the subscription is then placed at each such
//! source, notifying back to this broker, and every notification that arrives
//! under a forwarded copy's identifier is re-assembled and delivered to the
//! local subscriber under the local subscription's identifier.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

/// The companion context source registration subscription of a local subscription.
pub mod companion;
/// Failures of the distributed subscription side.
pub mod error;
/// Placing, updating and removing reduced copies at context sources.
pub mod forwarder;
/// The identifier mappings the distributed side keeps.
pub mod mapping;
/// Re-assembling an inbound notification for the local subscriber.
pub mod reassembly;
/// The notification endpoint this broker advertises to context sources.
pub mod receiver;
/// Reducing a subscription to what one registration matches.
pub mod reduced_copy;
/// The service tying the parts together.
pub mod service;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-delivery",
        "andromeda-entity",
        "andromeda-federation",
        "andromeda-jsonld",
        "andromeda-ngsi-ld-lexicon",
        "andromeda-ngsi-ld-model",
        "andromeda-ngsi-ld-render",
        "andromeda-ngsi-ld-representation",
        "andromeda-ngsi-ld-request",
        "andromeda-subscription",
        "andromeda-tenancy",
    ],
    allow_workspace_dev_dependencies: [
    ],
    allow_production_dependencies: [],
}

#[cfg(test)]
andromeda_architecture_checks::forbid_sql!();
