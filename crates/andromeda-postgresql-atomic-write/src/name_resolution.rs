//! The dictionary keys one commit's writes are stored under.
//!
//! An entity's search row carries its types as `entity_type_names` keys and
//! the names of its attributes as `attribute_names` keys, and its
//! `current_attributes` rows carry every attribute name at every depth as an
//! `attribute_names` key and every datatype, `valueType` and `objectType` IRI
//! as an `entity_type_names` key, so both dictionaries are consulted inside
//! the commit's own transaction before the statements that bind the keys run. Minting rather
//! than looking up: a type or attribute recorded for the first time has no
//! key yet, and the row that references it is written moments later in the
//! same transaction, so a key exists exactly when the row that references it
//! does — and vanishes with it on rollback.

use crate::commit_refusal::name_lookup;
use andromeda_context_commit::{atomic_write::AtomicWrite, error::CommitError};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName};
use andromeda_postgresql_context_store::current_attribute::name_keys::{MentionedNames, NameKeys};
use andromeda_postgresql_name_dictionary::name_lookup::NameLookup;
use andromeda_postgresql_query::dictionary::resolved_names::ResolvedNames;
use sqlx::{Postgres, Transaction};
use std::collections::HashMap;

/// Mint the dictionary key of every name the writes' records project or
/// store: every entity type and attribute name the search row carries, every
/// name the `current_attributes` rows of the state hold as a key
/// ([`MentionedNames`]), and the name of every attribute an update removes,
/// whose rows it deletes by key.
///
/// Only writes that carry an entity record contribute: a delete or a
/// history-only write states no types and stores no attribute. Resolved before
/// the entity statements run because the fused insert and update bind the
/// keys directly.
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
) -> Result<NameKeys, CommitError> {
    let mut mentioned = MentionedNames::default();
    for record in writes.iter().filter_map(|write| write.entity.record()) {
        // clone: the dictionary request owns the names it resolves.
        mentioned.types.extend(record.projection.types.iter().cloned());
        mentioned.attributes.extend(record.projection.attributes.names().iter().cloned());
        mentioned.collect(record.state.attributes());
        if let Some(replaced) = &record.replaces {
            let attributes = record.state.attributes();
            // clone: the dictionary request owns the names it resolves.
            mentioned
                .attributes
                .extend(replaced.attributes().names().filter(|name| !attributes.contains(name)).cloned());
        }
    }
    let MentionedNames { mut attributes, mut types } = mentioned;
    types.sort_unstable();
    types.dedup();
    attributes.sort_unstable();
    attributes.dedup();
    let types = types;
    let attributes = attributes;

    let types = if types.is_empty() {
        HashMap::new()
    } else {
        entity_type_names.resolve_or_create(tx, &types).await.map_err(|error| name_lookup(&error))?
    };
    let attributes = if attributes.is_empty() {
        HashMap::new()
    } else {
        attribute_names.resolve_or_create(tx, &attributes).await.map_err(|error| name_lookup(&error))?
    };
    Ok(NameKeys::new(ResolvedNames::from_pairs(attributes), ResolvedNames::from_pairs(types)))
}
