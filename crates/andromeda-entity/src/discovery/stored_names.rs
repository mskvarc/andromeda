//! Reading back the names the broker itself stored.
//!
//! A stored type name, attribute name or discovery IRI came out of the broker's
//! own store or its own compactor, so one that does not parse is an internal
//! fault rather than a client error.

use crate::error::EntityError;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName};
use iri_rs::IriBuf;
use std::str::FromStr;

/// Parse a stored entity-type name.
pub(super) fn parse_type_name(value: &str) -> Result<EntityTypeName, EntityError> {
    EntityTypeName::from_str(value).map_err(|error| EntityError::internal(format!("stored entity type name is not a valid NGSI-LD name: {error}")))
}

/// Parse a stored attribute name.
pub(super) fn parse_attribute_name(value: &str) -> Result<AttributeName, EntityError> {
    AttributeName::from_str(value).map_err(|error| EntityError::internal(format!("stored attribute name is not a valid NGSI-LD name: {error}")))
}

/// Parse a stored expanded IRI.
pub(super) fn parse_discovery_iri(value: String) -> Result<IriBuf, EntityError> {
    IriBuf::new(value).map_err(|error| EntityError::internal(format!("stored discovery identifier is not a valid IRI: {error}")))
}

#[cfg(test)]
mod tests {
    use crate::{
        discovery::stored_names::{parse_attribute_name, parse_discovery_iri, parse_type_name},
        error::EntityError,
    };

    #[test]
    fn a_stored_name_that_does_not_parse_is_an_internal_fault() {
        assert!(matches!(parse_type_name("two words"), Err(EntityError::Internal(_))));
        assert!(matches!(parse_attribute_name(""), Err(EntityError::Internal(_))));
        assert!(matches!(parse_discovery_iri("not an iri".to_owned()), Err(EntityError::Internal(_))));
    }
}
