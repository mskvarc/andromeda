//! What one information block of a registration covers (ETSI GS CIM 009 clause 5.2.10).

use crate::entity_info::EntityInfo;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier_length::IdentifierTooLong};
use serde::{Deserialize, Serialize};
use strum::{Display, IntoStaticStr};
use thiserror::Error;

/// Information block describing what a context source provides (ETSI GS CIM 009 clause 5.2.10).
///
/// Table 5.2.10-1 forbids an empty array on every member and requires at least
/// one member to be present, so a wire document violating either is rejected at
/// deserialisation rather than stored as a block that covers nothing.
///
/// An `idPattern` is also held to the identifier bound: the routing projection
/// keys a B-tree index on it, which refuses an entry that long, and clause
/// 5.2.8 sets no length of its own.
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

/// The three array members of a [`RegistrationInfo`] (Table 5.2.10-1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Display, IntoStaticStr)]
#[strum(serialize_all = "camelCase")]
pub enum RegistrationInfoMember {
    /// The `entities` member.
    Entities,
    /// The `propertyNames` member.
    PropertyNames,
    /// The `relationshipNames` member.
    RelationshipNames,
}

/// Why a wire document cannot be a [`RegistrationInfo`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum RegistrationInfoError {
    /// None of `entities`, `propertyNames` or `relationshipNames` was present.
    #[error("a RegistrationInfo must carry at least one of entities, propertyNames or relationshipNames")]
    Empty,
    /// A member was present as an empty array.
    #[error("RegistrationInfo member {0} must not be an empty array")]
    EmptyMember(RegistrationInfoMember),
    /// An `idPattern` is longer than the identifier bound.
    #[error("a RegistrationInfo idPattern is not storable: {0}")]
    IdPatternTooLong(#[from] IdentifierTooLong),
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
            return Err(RegistrationInfoError::EmptyMember(RegistrationInfoMember::Entities));
        }
        if wire.property_names.as_ref().is_some_and(Vec::is_empty) {
            return Err(RegistrationInfoError::EmptyMember(RegistrationInfoMember::PropertyNames));
        }
        if wire.relationship_names.as_ref().is_some_and(Vec::is_empty) {
            return Err(RegistrationInfoError::EmptyMember(RegistrationInfoMember::RelationshipNames));
        }
        wire.entities
            .iter()
            .flatten()
            .filter_map(|entity| entity.id_pattern.as_ref())
            .try_for_each(|pattern| IdentifierTooLong::check(pattern.as_str()))?;
        Ok(RegistrationInfo {
            entities: wire.entities,
            property_names: wire.property_names,
            relationship_names: wire.relationship_names,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::registration::info::{RegistrationInfo, RegistrationInfoError};
    use andromeda_ngsi_ld_lexicon::identifier_length::{IDENTIFIER_BYTE_LIMIT, IdentifierTooLong};
    use serde_json::json;

    #[test]
    fn an_id_pattern_at_the_limit_is_admitted() {
        let pattern = format!("urn:ngsi-ld:Vehicle:{}", "a".repeat(IDENTIFIER_BYTE_LIMIT - "urn:ngsi-ld:Vehicle:".len()));
        let info: RegistrationInfo =
            serde_json::from_value(json!({"entities": [{"type": "Vehicle", "idPattern": pattern}]})).expect("an idPattern at the limit");
        assert!(info.entities.is_some());
    }

    #[test]
    fn an_id_pattern_past_the_limit_is_refused() {
        let pattern = format!("urn:ngsi-ld:Vehicle:{}", "a".repeat(IDENTIFIER_BYTE_LIMIT));
        let error =
            serde_json::from_value::<RegistrationInfo>(json!({"entities": [{"type": "Vehicle", "idPattern": pattern}]})).expect_err("an over-long idPattern");
        let expected = RegistrationInfoError::IdPatternTooLong(IdentifierTooLong { length: pattern.len() }).to_string();
        assert!(error.to_string().contains(&expected), "{error}");
    }
}
