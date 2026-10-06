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
/// The invariants the distributed subscription side relies on.
pub mod forwarding_invariant;
/// The notifications received under identifiers this broker minted.
pub mod inbound;
/// The identifier mappings the distributed side keeps.
pub mod mapping;
/// The identifier this broker mints for a subscription it hands to a peer.
mod minted_subscription_id;
/// Narrowing a re-assembled entity to what the local subscription delivers.
mod notification_narrowing;
/// Re-assembling an inbound notification for the local subscriber.
pub mod reassembly;
/// The distributed subscription side wired to in-memory doubles.
#[cfg(test)]
mod reassembly_harness;
/// What the distributed subscription side does with an inbound notification.
#[cfg(test)]
mod reassembly_tests;
/// One entity of an inbound notification in the typed model.
mod received_entity;
/// The notification endpoint this broker advertises to context sources.
pub mod receiver;
/// Reducing a subscription to what one registration matches.
pub mod reduced_copy;
/// Tearing down the copies held at a removed registration's context source.
mod registration_removal;
/// The Linked Entities the local subscription's `join` reaches from a relayed
/// notification.
mod relayed_join;
/// What a relayed notification carries of its Linked Entities.
#[cfg(test)]
mod relayed_join_tests;
/// The service tying the parts together.
pub mod service;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-context-space",
        "andromeda-delivery",
        "andromeda-entity",
        "andromeda-federation",
        "andromeda-json-literal",
        "andromeda-jsonld",
        "andromeda-linked-retrieval",
        "andromeda-ngsi-ld-compaction",
        "andromeda-ngsi-ld-entity",
        "andromeda-ngsi-ld-expansion",
        "andromeda-ngsi-ld-lexicon",
        "andromeda-ngsi-ld-model",
        "andromeda-ngsi-ld-number",
        "andromeda-ngsi-ld-query",
        "andromeda-ngsi-ld-render",
        "andromeda-ngsi-ld-representation",
        "andromeda-ngsi-ld-request",
        "andromeda-persistence-failure",
        "andromeda-registration",
        "andromeda-subscription",
        "andromeda-tenancy",
    ],
    allow_workspace_dev_dependencies: [
        "andromeda-context-commit",
        "andromeda-test-mocks",
    ],
    allow_production_dependencies: [],
}

#[cfg(test)]
andromeda_architecture_checks::forbid_sql!();
