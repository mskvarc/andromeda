//! Row → domain conversions for the `entities` table.
//!
//! Entity payloads are stored as opaque JSONB `Value` blobs, so a payload needs
//! no conversion. The identifier, type-name and attribute-name columns do: they
//! come back as text and are lifted into their domain types here, once, so every
//! statement that reads them agrees on how a malformed one is reported. The
//! broker only ever writes valid values, so a parse failure is an internal
//! fault rather than client input.

use crate::{
    entity::{
        inserted_entity::{InsertedEntity, MintedEntityKey},
        written_entity::WrittenEntity,
    },
    error::ContextStoreError,
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, uri::Uri};
use andromeda_postgresql_codes::entity_key::EntityKey;
use std::str::FromStr;

/// Parse a stored entity identifier into a [`Uri`].
///
/// # Errors
///
/// Returns [`ContextStoreError::Internal`] when the stored text is not a valid
/// URI, which means the column holds a value the broker never wrote.
pub fn stored_uri(raw: &str) -> Result<Uri, ContextStoreError> {
    Uri::from_str(raw).map_err(|error| ContextStoreError::Internal(format!("stored entity identifier is not a valid URI: {error}")))
}

/// Parse a stored surrogate key into an [`EntityKey`].
///
/// # Errors
///
/// Returns [`ContextStoreError::Internal`] when the stored key is not positive,
/// which the identity column's generated-always constraint makes impossible.
pub fn stored_entity_key(raw: i64) -> Result<EntityKey, ContextStoreError> {
    EntityKey::try_from(raw).map_err(|error| ContextStoreError::Internal(format!("stored entity key is not a valid surrogate key: {error}")))
}

/// Lift the identity pair a write statement returns into a [`WrittenEntity`].
///
/// # Errors
///
/// Returns [`ContextStoreError::Internal`] when the returned key is not a valid
/// surrogate key or the returned identifier is not a valid URI.
pub fn stored_written_entity(key: i64, id: &str) -> Result<WrittenEntity, ContextStoreError> {
    Ok(WrittenEntity {
        key: stored_entity_key(key)?,
        id: stored_uri(id)?,
    })
}

/// Build the [`InsertedEntity`] the fused insert's `RETURNING` reports for one
/// entity it created.
///
/// # Errors
///
/// Returns [`ContextStoreError::Internal`] when the returned key is not a valid
/// surrogate key or the returned identifier is not a valid URI.
pub(crate) fn stored_inserted_entity(key: i64, id: &str) -> Result<InsertedEntity, ContextStoreError> {
    Ok(InsertedEntity {
        key: MintedEntityKey::new(stored_entity_key(key)?),
        id: stored_uri(id)?,
    })
}

/// Parse a stored entity type IRI into an [`EntityTypeName`].
///
/// # Errors
///
/// Returns [`ContextStoreError::Internal`] when the stored text is not a valid
/// NGSI-LD name, which means the dictionary holds a value the broker never
/// interned.
pub fn stored_type_name(raw: &str) -> Result<EntityTypeName, ContextStoreError> {
    EntityTypeName::from_str(raw).map_err(|error| ContextStoreError::Internal(format!("stored entity type name is not a valid NGSI-LD name: {error}")))
}

/// Parse a stored attribute IRI into an [`AttributeName`].
///
/// # Errors
///
/// Returns [`ContextStoreError::Internal`] when the stored text is not a valid
/// NGSI-LD name, which means the dictionary holds a value the broker never
/// interned.
pub fn stored_attribute_name(raw: &str) -> Result<AttributeName, ContextStoreError> {
    AttributeName::from_str(raw).map_err(|error| ContextStoreError::Internal(format!("stored attribute name is not a valid NGSI-LD name: {error}")))
}
