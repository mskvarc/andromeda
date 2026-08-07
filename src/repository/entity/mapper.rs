//! Row → domain conversions for the `entities` table.
//!
//! Entity payloads are stored as opaque JSONB `Value` blobs, so a payload needs
//! no conversion. The identifier, type-name and attribute-name columns do: they
//! come back as text and are lifted into their domain types here, once, so every
//! statement that reads them agrees on how a malformed one is reported. The
//! broker only ever writes valid values, so a parse failure is an internal
//! fault rather than client input.

use crate::core::error::AndromedaError;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, uri::Uri};
use std::str::FromStr;

/// Parse a stored entity identifier into a [`Uri`].
pub fn stored_uri(raw: String) -> Result<Uri, AndromedaError> {
    Uri::from_str(&raw).map_err(|error| AndromedaError::internal(format!("stored entity identifier is not a valid URI: {error}")))
}

/// Parse a stored entity type IRI into an [`EntityTypeName`].
pub fn stored_type_name(raw: String) -> Result<EntityTypeName, AndromedaError> {
    EntityTypeName::from_str(&raw).map_err(|error| AndromedaError::internal(format!("stored entity type name is not a valid NGSI-LD name: {error}")))
}

/// Parse a stored attribute IRI into an [`AttributeName`].
pub fn stored_attribute_name(raw: String) -> Result<AttributeName, AndromedaError> {
    AttributeName::from_str(&raw).map_err(|error| AndromedaError::internal(format!("stored attribute name is not a valid NGSI-LD name: {error}")))
}
