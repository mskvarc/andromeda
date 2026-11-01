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
/// What an expired subscription leaves at the context sources.
#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod expiry_withdrawal_tests;
/// Placing, updating and removing reduced copies at context sources.
pub mod forwarder;
/// The invariants the distributed subscription side relies on.
pub mod forwarding_invariant;
/// The notifications received under identifiers this broker minted.
pub mod inbound;
/// What a source whose registration starts to match is given.
#[cfg(test)]
#[expect(
    clippy::expect_used,
    clippy::panic,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod late_placement_tests;
/// The lifecycle steps of one local subscription, run one after the other.
#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod lifecycle_step_tests;
/// The identifier mappings the distributed side keeps.
pub mod mapping;
/// The identifier this broker mints for a subscription it hands to a peer.
mod minted_subscription_id;
/// Narrowing a re-assembled entity to what the local subscription delivers.
mod notification_narrowing;
/// Whether a local subscription holds a companion and forwarded copies.
mod participation;
/// Re-assembling an inbound notification for the local subscriber.
pub mod reassembly;
/// The distributed subscription side wired to in-memory doubles.
#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod reassembly_harness;
/// What the distributed subscription side does with an inbound notification.
#[cfg(test)]
#[expect(
    clippy::expect_used,
    clippy::panic,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod reassembly_tests;
/// One entity of an inbound notification in the typed model.
mod received_entity;
/// The notification endpoint this broker advertises to context sources.
pub mod receiver;
/// Reducing a subscription to what one registration matches.
pub mod reduced_copy;
/// Tearing down the copies held at a removed registration's context source.
mod registration_removal;
/// Relaying one notified entity as the inbound side does, for the tests.
#[cfg(test)]
#[expect(
    clippy::expect_used,
    clippy::panic,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod relay_fixture;
/// What a relayed notification makes of the deletions its source reports.
#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod relayed_deletion_tests;
/// The entities of a relayed copy beside what their source reported of the
/// change.
mod relayed_entities;
/// The Linked Entities the local subscription's `join` reaches from a relayed
/// notification.
mod relayed_join;
/// What a relayed notification carries of its Linked Entities.
#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod relayed_join_tests;
/// The service tying the parts together.
pub mod service;
/// Tearing down what a local subscription left at the context sources.
mod teardown;

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
    allow_workspace_dev_dependencies: ["andromeda-configuration", "andromeda-context-commit"],
    allow_production_dependencies: [],
}

#[cfg(test)]
andromeda_architecture_checks::forbid_sql!();
