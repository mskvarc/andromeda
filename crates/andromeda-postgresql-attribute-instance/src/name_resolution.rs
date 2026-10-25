//! Reaching the dictionaries for the keys a write flattens with and the names
//! a read assembles with.
//!
//! A write mints the key of every name [`MentionedNames`] collects inside its
//! own transaction, on the connection that transaction holds: a key a stored
//! row references is written by the same transaction as the row, so it exists
//! exactly when the row does and vanishes with it on rollback. A read looks up
//! the names behind every key [`HeldKeys`] collects once per dictionary, on
//! the dictionaries' own pool, before it assembles.

use crate::name_keys::{HeldKeys, KeyNames, MentionedNames, NameKeys};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, reference::NgsiLdReference};
use andromeda_postgresql_name_dictionary::name_lookup::{NameLookup, NameLookupError};
use andromeda_postgresql_query::dictionary::resolved_names::ResolvedNames;
use sqlx::PgConnection;
use std::collections::HashMap;

/// Mint the dictionary key of every name `mentioned` collects, in
/// `connection`'s transaction, one resolution per dictionary, each name asked
/// for once.
///
/// # Errors
///
/// Returns [`NameLookupError`] when a dictionary cannot be resolved.
pub async fn resolve_mentioned(
    connection: &mut PgConnection,
    mentioned: MentionedNames,
    entity_type_names: &dyn NameLookup<EntityTypeName>,
    attribute_names: &dyn NameLookup<AttributeName>,
) -> Result<NameKeys, NameLookupError> {
    let MentionedNames { attributes, types } = distinct(mentioned);
    let types = if types.is_empty() {
        HashMap::new()
    } else {
        entity_type_names.resolve_or_create(&mut *connection, &types).await?
    };
    let attributes = if attributes.is_empty() {
        HashMap::new()
    } else {
        attribute_names.resolve_or_create(connection, &attributes).await?
    };
    Ok(NameKeys::new(ResolvedNames::from_pairs(attributes), ResolvedNames::from_pairs(types)))
}

/// The names `mentioned` collects, each once, in sorted order.
#[must_use]
pub fn distinct(mentioned: MentionedNames) -> MentionedNames {
    let MentionedNames { mut attributes, mut types } = mentioned;
    types.sort_unstable();
    types.dedup();
    attributes.sort_unstable();
    attributes.dedup();
    MentionedNames { attributes, types }
}

/// The names behind `held`, one lookup per dictionary.
///
/// # Errors
///
/// Returns [`NameLookupError`] when a dictionary cannot be read.
pub async fn names_of(
    held: &HeldKeys,
    attribute_names: &dyn NameLookup<AttributeName>,
    entity_type_names: &dyn NameLookup<EntityTypeName>,
) -> Result<KeyNames, NameLookupError> {
    let attributes = attribute_names.names_of(&held.attributes).await?;
    let types = entity_type_names.names_of(&held.types).await?;
    Ok(KeyNames::from_pairs(
        attributes,
        types.iter().map(|(key, name)| (*key, NgsiLdReference::from(name))),
    ))
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{name_keys::MentionedNames, name_resolution::distinct};

    #[test]
    fn every_mentioned_name_is_asked_for_once_in_sorted_order() {
        let attribute = |raw: &str| raw.parse().expect("an attribute name");
        let entity_type = |raw: &str| raw.parse().expect("an entity type name");
        let mentioned = MentionedNames {
            attributes: vec![attribute("https://ex.org/b"), attribute("https://ex.org/a"), attribute("https://ex.org/b")],
            types: vec![entity_type("https://ex.org/T"), entity_type("https://ex.org/T")],
        };
        let MentionedNames { attributes, types } = distinct(mentioned);
        pretty_assertions::assert_eq!(attributes, vec![attribute("https://ex.org/a"), attribute("https://ex.org/b")]);
        pretty_assertions::assert_eq!(types, vec![entity_type("https://ex.org/T")]);
    }
}
