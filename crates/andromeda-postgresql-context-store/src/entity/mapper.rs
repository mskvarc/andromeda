//! Row → domain conversions for the `entities` table.
//!
//! An entity payload is the stored expanded document, decoded by the entity
//! state codec straight from the column's JSON text. The identifier, type-name
//! and attribute-name columns come back as text and are lifted into their
//! domain types here, once, so every statement that reads them agrees on how a
//! malformed one is reported. The broker only ever writes valid values, so a
//! decode or parse failure is an internal fault rather than client input.

use crate::{
    entity::{
        inserted_entity::{InsertedEntity, MintedEntityKey},
        stored_entity_error::StoredEntityError,
        written_entity::WrittenEntity,
    },
    error::ContextStoreError,
    jsonb_text::JsonbText,
};
use andromeda_ngsi_ld_entity::entity_state::EntityState;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType, entity_type_name::EntityTypeName, identifier::Identifier};
use andromeda_postgresql_codes::{attribute_kind::AttributeKind, attribute_kind_code::attribute_type_of, entity_key::EntityKey};
use std::str::FromStr;

/// Decode a stored entity document into the [`EntityState`] it was written
/// from.
///
/// # Errors
///
/// Returns [`ContextStoreError::StoredEntity`] when the stored document does
/// not decode as an entity state, which means the column holds a document the
/// broker never wrote.
pub fn stored_state(payload: JsonbText<'_>) -> Result<EntityState, ContextStoreError> {
    Ok(EntityState::from_slice(payload.as_bytes()).map_err(StoredEntityError::Payload)?)
}

/// Parse a stored entity identifier into a [`Identifier`].
///
/// # Errors
///
/// Returns [`ContextStoreError::StoredEntity`] when the stored text is not a
/// valid URI, which means the column holds a value the broker never wrote.
pub fn stored_uri(raw: &str) -> Result<Identifier, ContextStoreError> {
    Ok(Identifier::from_str(raw).map_err(StoredEntityError::Id)?)
}

/// Parse a stored surrogate key into an [`EntityKey`].
///
/// # Errors
///
/// Returns [`ContextStoreError::StoredEntity`] when the stored key is not
/// positive, which the identity column's generated-always constraint makes
/// impossible.
pub fn stored_entity_key(raw: i64) -> Result<EntityKey, ContextStoreError> {
    Ok(EntityKey::try_from(raw).map_err(StoredEntityError::Key)?)
}

/// Lift the identity pair a write statement returns into a [`WrittenEntity`].
///
/// # Errors
///
/// Returns [`ContextStoreError::StoredEntity`] when the returned key is not a
/// valid surrogate key or the returned identifier is not a valid URI.
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
/// Returns [`ContextStoreError::StoredEntity`] when the returned key is not a
/// valid surrogate key or the returned identifier is not a valid URI.
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
/// Returns [`ContextStoreError::StoredEntity`] when the stored text is not a
/// valid NGSI-LD name, which means the dictionary holds a value the broker
/// never interned.
pub fn stored_type_name(raw: &str) -> Result<EntityTypeName, ContextStoreError> {
    Ok(EntityTypeName::from_str(raw).map_err(StoredEntityError::TypeName)?)
}

/// Parse a stored attribute IRI into an [`AttributeName`].
///
/// # Errors
///
/// Returns [`ContextStoreError::StoredEntity`] when the stored text is not a
/// valid NGSI-LD name, which means the dictionary holds a value the broker
/// never interned.
pub fn stored_attribute_name(raw: &str) -> Result<AttributeName, ContextStoreError> {
    Ok(AttributeName::from_str(raw).map_err(StoredEntityError::AttributeName)?)
}

/// Read a stored attribute family code back into the [`AttributeType`] it
/// names.
///
/// # Errors
///
/// Returns [`ContextStoreError::StoredEntity`] when the code is not one the
/// registry assigned.
pub fn stored_attribute_family(code: i16) -> Result<AttributeType, ContextStoreError> {
    AttributeKind::try_from(code)
        .map(attribute_type_of)
        .map_err(|error| StoredEntityError::AttributeFamily(error).into())
}
