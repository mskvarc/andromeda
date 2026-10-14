//! The encodings Andromeda's PostgreSQL schema persists rows by.
//!
//! Two things a stored row is made of live here, and nothing else does. The
//! first is the compact code domains: the small closed vocabularies a migration
//! writes into `SMALLINT` columns — the attribute family and value shape of a
//! history row, the trigger conditions of a subscription, the lifecycle and
//! instance transitions of a notification event, the origin of an owed
//! change, the operation set of a context-source registration. The second is the identities those rows are
//! keyed and referenced by: the current-state entity key, the temporal entity
//! key and the attribute stream key.
//!
//! The split instance identity is deliberately not here. Clause 4.5.7 makes an
//! `instanceId` part of what a client is handed back, so it is a domain value
//! the lexicon owns and every layer above storage speaks; a second copy keyed
//! to one backend would be a second spelling of one concept.
//!
//! They belong together because they share one obligation that ordinary domain
//! types do not carry. A code or an identity written by one release is read
//! back by every later one, from rows nobody will rewrite, so its meaning is
//! fixed by the migration that introduced it rather than by whatever the
//! current Rust source happens to say. That is why numbers are written as
//! literals instead of being left to the compiler, why assignment is
//! append-only, and why an unrecognised value fails to decode rather than
//! defaulting.
//!
//! Nothing here spells SQL. The crate names what a stored value *is*; how a
//! statement selects or writes it belongs to the store adapters.
//!
//! See [`registry`] for the pattern every code domain is declared with.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

/// The persisted transition of one Attribute instance a notification event
/// records.
pub mod attribute_change_kind;
/// The persisted attribute family of one history row.
pub mod attribute_kind;
/// Translation between the NGSI-LD attribute family and its stored code.
pub mod attribute_kind_code;
/// The persisted origin of one change a respond-async job owes.
pub mod change_origin;
/// The closed catalogue of compact code domains an unreadable number is reported against.
pub mod compact_code_domain;
/// The persisted distributed-operation mode of one registration.
pub mod csr_mode;
/// The persisted operation set of one context-source registration.
pub mod csr_operation;
/// The persisted health of one registration's context source.
pub mod csr_status;
/// The narrow key a current-state entity is referenced by.
pub mod entity_key;
/// The persisted lifecycle transition of one notification event.
pub mod entity_lifecycle;
/// The persisted shape of one registered `EntityInfo` type.
pub mod entity_type_shape;
/// The persisted kind of one stored JSON-LD context.
pub mod jsonld_context_kind;
/// The persisted outcome of one maintenance action.
pub mod maintenance_action_result;
/// The persisted policy decision of one maintenance target.
pub mod maintenance_decision;
/// The persisted outcome of one tenant's maintenance inspection.
pub mod maintenance_inspection_result;
/// The persisted relation one maintenance target row describes.
pub mod maintenance_target;
/// The persisted trigger conditions a subscription listens for.
pub mod notification_trigger;
/// The one pattern every migration-owned compact code domain is declared with.
pub mod registry;
/// The persisted value shape of one scope-history row.
pub mod scope_value_kind;
/// The persisted lifecycle state of one snapshot.
pub mod snapshot_status;
/// The narrow key a temporal attribute stream is referenced by.
pub mod stream_key;
/// The persisted family of one stored subscription.
pub mod subscription_kind;
/// The persisted lifecycle state of one stored subscription.
pub mod subscription_status;
/// The narrow key a temporal entity is referenced by.
pub mod temporal_entity_key;
/// The persisted kind of one entry of a temporal append intent.
pub mod temporal_entry_kind;
/// The persisted needs of an intent application one queued intent meets.
pub mod temporal_intent_reach;
/// The persisted administrative status of one tenant.
pub mod tenant_status;
/// The persisted value shape of one attribute-history row.
pub mod value_kind;

#[cfg(test)]
andromeda_architecture_checks::forbid_sql!();

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: ["andromeda-ngsi-ld-lexicon"],
    allow_workspace_dev_dependencies: [],
    allow_production_dependencies: [],
}
