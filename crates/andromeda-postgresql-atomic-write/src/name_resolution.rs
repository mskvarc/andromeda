//! The dictionary keys one commit's writes are stored under.
//!
//! An entity's search row carries its types as `entity_type_names` keys and
//! the names of its attributes as `attribute_names` keys, and the
//! current-attribute projection carries its attributes as `attribute_names`
//! keys too, so both dictionaries are consulted inside the commit's own
//! transaction before the statements that bind the keys run. Minting rather
//! than looking up: a type or attribute recorded for the first time has no
//! key yet, and the row that references it is written moments later in the
//! same transaction, so a key exists exactly when the row that references it
//! does — and vanishes with it on rollback.

use crate::commit_refusal::persistence;
use andromeda_context_commit::{atomic_write::AtomicWrite, error::CommitError};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName};
use andromeda_postgresql_name_dictionary::name_lookup::NameLookup;
use andromeda_postgresql_query::{attribute_name_id::AttributeNameId, entity_type_id::EntityTypeId};
use andromeda_temporal::append_intent::{TemporalAppendEntry, TemporalAppendIntent};
use sqlx::{Postgres, Transaction};
use std::collections::HashMap;

/// The dictionary keys the entity statements of one commit bind: every type
/// and every attribute name the writes' records project.
#[derive(Debug, Default)]
pub(crate) struct ResolvedWriteNames {
    /// The key of every entity type a record declares.
    pub(crate) types: HashMap<EntityTypeName, EntityTypeId>,
    /// The key of every attribute name a record's payload carries.
    pub(crate) attributes: HashMap<AttributeName, AttributeNameId>,
}

/// Mint the dictionary key of every entity type and attribute name the
/// writes' records project.
///
/// Only writes that carry an entity record contribute: a delete or a
/// history-only write states no types and carries no payload. Resolved before
/// the entity statements run because the fused insert and update bind both
/// key arrays directly.
///
/// # Errors
///
/// Returns [`CommitError::Persistence`] when a dictionary cannot be resolved,
/// which includes a resolution refused for taking the tenant's dictionary past
/// its bound.
pub(crate) async fn resolve_write_names(
    tx: &mut Transaction<'_, Postgres>,
    writes: &[AtomicWrite],
    entity_type_names: &dyn NameLookup<EntityTypeName>,
    attribute_names: &dyn NameLookup<AttributeName>,
) -> Result<ResolvedWriteNames, CommitError> {
    let mut types: Vec<EntityTypeName> = Vec::new();
    let mut attributes: Vec<AttributeName> = Vec::new();
    for record in writes.iter().filter_map(|write| write.entity.record()) {
        // clone: the dictionary request owns the names it resolves.
        types.extend(record.projection.types.iter().cloned());
        attributes.extend(record.projection.attribute_names.iter().cloned());
    }
    types.sort_unstable();
    types.dedup();
    attributes.sort_unstable();
    attributes.dedup();
    let types = types;
    let attributes = attributes;

    let types = if types.is_empty() {
        HashMap::new()
    } else {
        entity_type_names.resolve_or_create(tx, &types).await.map_err(persistence)?
    };
    let attributes = if attributes.is_empty() {
        HashMap::new()
    } else {
        attribute_names.resolve_or_create(tx, &attributes).await.map_err(persistence)?
    };
    Ok(ResolvedWriteNames { types, attributes })
}

/// Mint the dictionary key of every attribute the interpreted intents name.
///
/// Resolved after the mutation is interpreted, from the intents themselves,
/// so only attributes a landed write actually observed or tombstoned mint a
/// key; the projection's foreign key demands one for every row it writes.
///
/// # Errors
///
/// Returns [`CommitError::Persistence`] when the dictionary cannot be resolved,
/// which includes a resolution refused for taking the tenant's dictionary past
/// its bound.
pub(crate) async fn resolve_attribute_names(
    tx: &mut Transaction<'_, Postgres>,
    intents: &[TemporalAppendIntent],
    attribute_names: &dyn NameLookup<AttributeName>,
) -> Result<HashMap<AttributeName, AttributeNameId>, CommitError> {
    let mut names: Vec<AttributeName> = intents
        .iter()
        .flat_map(|intent| intent.entries.iter())
        .filter_map(|entry| match entry {
            // clone: the dictionary request owns the names it resolves.
            TemporalAppendEntry::Observation { row } => Some(row.attribute_name.clone()),
            TemporalAppendEntry::Tombstone { attribute_name, .. } => Some(attribute_name.clone()),
            TemporalAppendEntry::Scope { .. } => None,
        })
        .collect();
    names.sort_unstable();
    names.dedup();
    let names = names;
    if names.is_empty() {
        return Ok(HashMap::new());
    }
    attribute_names.resolve_or_create(tx, &names).await.map_err(persistence)
}
