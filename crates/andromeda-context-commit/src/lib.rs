//! Atomic context-write policy and the port that persists it.
//!
//! One [`AtomicWrite`] describes the entity mutation and every durable
//! consequence that must share its commit. [`ContextCommitter`] is deliberately
//! one operation: transaction handles, statement phases and backend encodings
//! belong to its implementations.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

/// Everything one entity write must commit together.
pub mod atomic_write;
/// Deferred temporal and change-event consequences of a batch write.
pub mod batch_side_effect_job;
/// The entity change event consumed after a commit.
pub mod change_event;
/// The atomic commit port and submission semantics.
pub mod committer;
/// An entity captured immediately before deletion.
pub mod deleted_entity;
/// Entity-row mutations and their semantic payload.
pub mod entity_mutation;
/// Failures reported by the atomic commit boundary.
pub mod error;
/// Temporal-history consequences carried by a write.
pub mod history_record;
/// Notification outbox data known at commit time.
pub mod new_outbox_entry;
/// Preconditions and retry classification for entity mutations.
pub mod precondition;
/// Per-entity atomic write outcomes.
pub mod write_outcome;

#[cfg(test)]
mod boundary {
    /// The commit policy may depend only on specification/domain crates and its
    /// test-only layer guard.
    #[test]
    fn this_crate_declares_only_what_its_layer_permits() {
        /// In-workspace crates this one may name in any dependency table.
        const ALLOWED_IN_WORKSPACE: &[&str] = &[
            "andromeda-layer-guard",
            "andromeda-ngsi-ld-geometry",
            "andromeda-ngsi-ld-lexicon",
            "andromeda-ngsi-ld-model",
        ];
        let forbidden = andromeda_layer_guard::manifest::baseline_forbidden_except(&[]);
        andromeda_layer_guard::manifest::assert_layer(env!("CARGO_MANIFEST_DIR"), &forbidden, ALLOWED_IN_WORKSPACE);
    }
}
