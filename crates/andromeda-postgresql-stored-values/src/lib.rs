//! The encodings Andromeda's PostgreSQL schema persists rows by.
//!
//! Two things a stored row is made of live here, and nothing else does. The
//! first is the compact code domains: the small closed vocabularies a migration
//! writes into `SMALLINT` columns: the attribute family and value shape of a
//! history row, the trigger conditions of a subscription, the lifecycle and
//! instance transitions of a notification event, the origin of an owed
//! change, the operation set of a context-source registration. Beside the
//! value shape sits the field each shape is held in. The second is the
//! identities and encodings those rows are keyed and referenced by: the
//! entity key, the attribute stream key, the unit-code identity the
//! `cefact-units` library assigns, the dictionary keys attribute and type names
//! are held as (with the name type each key stands for and the keys one
//! statement is rendered against), the grain key a `datasetId` is held as, the
//! equality keys of a sub-attribute tree, and the label path a Scope is stored
//! as.
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
//! statement selects or writes it belongs to the statement crates above it.
//!
//! See [`registry`] for the pattern every code domain is declared with.

/// The persisted transition of one Attribute instance a notification event
/// records.
pub mod attribute_change_kind;
/// The persisted attribute family of one history row.
pub mod attribute_kind;
/// Translation between the NGSI-LD attribute family and its stored code.
pub mod attribute_kind_code;
/// The key the `attribute_names` dictionary hands out for an expanded attribute
/// IRI.
pub mod attribute_name_key;
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
/// The grain key a stored attribute instance's `datasetId` is held as.
pub mod dataset_key;
/// The narrow key a current-state entity is referenced by.
pub mod entity_key;
/// The persisted lifecycle transition of one notification event.
pub mod entity_lifecycle;
/// The persisted operation that created one stored Entity Map.
pub mod entity_map_request_kind;
/// The key the `entity_type_names` dictionary hands out for an expanded type
/// IRI.
pub mod entity_type_name_key;
/// The persisted shape of one registered `EntityInfo` type.
pub mod entity_type_shape;
/// The name types a tenant dictionary interns, each under its own key type.
pub mod interned_name;
/// The persisted kind of one stored JSON-LD context.
pub mod jsonld_context_kind;
/// The persisted outcome of one maintenance action.
pub mod maintenance_action_result;
/// The persisted policy decision of one maintenance target.
pub mod maintenance_decision;
/// The persisted outcome of one tenant's maintenance inspection.
pub mod maintenance_inspection_result;
/// The persisted phase of one running table repack.
pub mod maintenance_repack_phase;
/// The persisted relation one maintenance target row describes.
pub mod maintenance_target;
/// The persisted trigger conditions a subscription listens for.
pub mod notification_trigger;
/// The persisted API of one stored posted query.
pub mod posted_query_kind;
/// The persisted refined value shape of one attribute-history instance.
pub mod recorded_shape;
/// The one pattern every migration-owned compact code domain is declared with.
pub mod registry;
/// The freeze every compact code registry declares, gathered for a check
/// against the seal ledger.
pub mod registry_freeze;
/// The dictionary keys a statement is rendered against.
pub mod resolved_names;
/// The persisted value shape of one scope-history row.
pub mod scope_value_kind;
/// The persisted lifecycle state of one snapshot.
pub mod snapshot_status;
/// The stored form of a clause 4.18 Scope.
pub mod stored_scope_path;
/// The stored form of a `unitCode`: the `cefact-units` identity of its code.
pub mod stored_unit_code;
/// The narrow key a temporal attribute stream is referenced by.
pub mod stream_key;
/// The equality keys of sub-attribute values.
pub mod sub_attribute_key;
/// The persisted family of one stored subscription.
pub mod subscription_kind;
/// The persisted lifecycle state of one stored subscription.
pub mod subscription_status;
/// The persisted kind of one entry of a temporal append intent.
pub mod temporal_entry_kind;
/// The persisted needs of an intent application one queued intent meets.
pub mod temporal_intent_reach;
/// The persisted administrative status of one tenant.
pub mod tenant_status;
/// The persisted value shape of one attribute-history row.
pub mod value_kind;
/// Where a stored instance holds its value, by value shape.
pub mod value_placement;

#[cfg(test)]
andromeda_architecture_checks::forbid_sql!();

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: ["andromeda-ngsi-ld-lexicon", "andromeda-postgresql-schema"],
    allow_workspace_dev_dependencies: [],
    allow_production_dependencies: [],
}
