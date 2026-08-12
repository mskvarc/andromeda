//! Row → domain conversions for the `entities` table.
//!
//! Entity payloads are stored as opaque JSONB `Value` blobs, so a payload needs
//! no conversion. The identifier, type-name and attribute-name columns do: they
//! come back as text and are lifted into their domain types here, once, so every
//! statement that reads them agrees on how a malformed one is reported. The
//! broker only ever writes valid values, so a parse failure is an internal
//! fault rather than client input.

use crate::{entity::written_entity::WrittenEntity, error::ContextStoreError};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, uri::Uri};
use andromeda_postgresql_codes::entity_key::EntityKey;
use std::str::FromStr;

/// Parse a stored entity identifier into a [`Uri`].
pub fn stored_uri(raw: String) -> Result<Uri, ContextStoreError> {
    Uri::from_str(&raw).map_err(|error| ContextStoreError::Internal(format!("stored entity identifier is not a valid URI: {error}")))
}

/// Parse a stored surrogate key into an [`EntityKey`].
pub fn stored_entity_key(raw: i64) -> Result<EntityKey, ContextStoreError> {
    EntityKey::try_from(raw).map_err(|error| ContextStoreError::Internal(format!("stored entity key is not a valid surrogate key: {error}")))
}

/// Lift the identity pair a write statement returns into a [`WrittenEntity`].
pub fn stored_written_entity(key: i64, id: String) -> Result<WrittenEntity, ContextStoreError> {
    Ok(WrittenEntity {
        key: stored_entity_key(key)?,
        id: stored_uri(id)?,
    })
}

/// Parse a stored entity type IRI into an [`EntityTypeName`].
pub fn stored_type_name(raw: String) -> Result<EntityTypeName, ContextStoreError> {
    EntityTypeName::from_str(&raw).map_err(|error| ContextStoreError::Internal(format!("stored entity type name is not a valid NGSI-LD name: {error}")))
}

/// Parse a stored attribute IRI into an [`AttributeName`].
pub fn stored_attribute_name(raw: String) -> Result<AttributeName, ContextStoreError> {
    AttributeName::from_str(&raw).map_err(|error| ContextStoreError::Internal(format!("stored attribute name is not a valid NGSI-LD name: {error}")))
}
