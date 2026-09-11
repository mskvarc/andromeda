//! What one information block of a registration covers (ETSI GS CIM 009 clause 5.2.10).

use crate::entity_info::EntityInfo;
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Information block describing what a context source provides (ETSI GS CIM 009 clause 5.2.10).
///
/// Table 5.2.10-1 forbids an empty array on every member and requires at least
/// one member to be present, so a wire document violating either is rejected at
/// deserialisation rather than stored as a block that covers nothing.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", try_from = "RegistrationInfoWire")]
pub struct RegistrationInfo {
    /// The entities this registration covers (clause 5.2.8).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entities: Option<Vec<EntityInfo>>,
    /// Property names provided by this context source.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub property_names: Option<Vec<AttributeName>>,
    /// Relationship names provided by this context source.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relationship_names: Option<Vec<AttributeName>>,
}

impl RegistrationInfo {
    /// Every attribute name the block declares, on both the property and the
    /// relationship axis.
    pub fn attribute_names(&self) -> impl Iterator<Item = &AttributeName> {
        self.property_names.iter().chain(self.relationship_names.iter()).flat_map(|names| names.iter())
    }
}

/// Why a wire document cannot be a [`RegistrationInfo`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum RegistrationInfoError {
    /// None of `entities`, `propertyNames` or `relationshipNames` was present.
    #[error("a RegistrationInfo must carry at least one of entities, propertyNames or relationshipNames")]
    Empty,
    /// A member was present as an empty array.
    #[error("RegistrationInfo member {0} must not be an empty array")]
    EmptyMember(&'static str),
}

/// The clause 5.2.10 wire shape before its cardinality restrictions are checked.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RegistrationInfoWire {
    /// The `entities` member.
    entities: Option<Vec<EntityInfo>>,
    /// The `propertyNames` member.
    property_names: Option<Vec<AttributeName>>,
    /// The `relationshipNames` member.
    relationship_names: Option<Vec<AttributeName>>,
}

impl TryFrom<RegistrationInfoWire> for RegistrationInfo {
    type Error = RegistrationInfoError;

    fn try_from(wire: RegistrationInfoWire) -> Result<RegistrationInfo, RegistrationInfoError> {
        if wire.entities.is_none() && wire.property_names.is_none() && wire.relationship_names.is_none() {
            return Err(RegistrationInfoError::Empty);
        }
        if wire.entities.as_ref().is_some_and(Vec::is_empty) {
            return Err(RegistrationInfoError::EmptyMember("entities"));
        }
        if wire.property_names.as_ref().is_some_and(Vec::is_empty) {
            return Err(RegistrationInfoError::EmptyMember("propertyNames"));
        }
        if wire.relationship_names.as_ref().is_some_and(Vec::is_empty) {
            return Err(RegistrationInfoError::EmptyMember("relationshipNames"));
        }
        Ok(RegistrationInfo {
            entities: wire.entities,
            property_names: wire.property_names,
            relationship_names: wire.relationship_names,
        })
    }
}
