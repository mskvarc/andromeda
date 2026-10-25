//! The entities a context source registers (ETSI GS CIM 009 clause 5.2.8).

use andromeda_ngsi_ld_lexicon::{entity_type_name::EntityTypeName, identifier::Identifier};
use andromeda_ngsi_ld_query::{pattern::Pattern, type_selection::grammar::TypeExprRaw};
use serde::{Deserialize, Serialize};
use std::slice;
use thiserror::Error;

/// The `type` of an `EntityInfo` (Table 5.2.8-1): one entity type, or a JSON
/// array of them for entities with several types.
///
/// Read back exactly as written: a registration made with a single name gives
/// the name back, one made with an array gives the array back. An entity
/// matches the selection when it carries any of the names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged, try_from = "EntityTypeSelectionWire")]
pub enum EntityTypeSelection {
    /// One entity type.
    Single(EntityTypeName),
    /// Several entity types, at least one.
    Several(Vec<EntityTypeName>),
}

impl EntityTypeSelection {
    /// Every type name the selection carries.
    #[must_use]
    pub fn names(&self) -> &[EntityTypeName] {
        match self {
            EntityTypeSelection::Single(name) => slice::from_ref(name),
            EntityTypeSelection::Several(names) => names,
        }
    }

    /// The selection as a clause 4.17 expression: the one name, or the
    /// alternation of the several.
    pub fn as_type_expression(&self) -> TypeExprRaw {
        match self {
            EntityTypeSelection::Single(name) => TypeExprRaw::Type(name.clone()), // clone: the expression owns its leaves
            EntityTypeSelection::Several(names) => TypeExprRaw::Or(names.iter().cloned().map(TypeExprRaw::Type).collect()), // clone: same
        }
    }
}

/// Why a wire document cannot be an [`EntityTypeSelection`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum EntityTypeSelectionError {
    /// The array form named no type at all.
    #[error("an EntityInfo type array must name at least one type")]
    Empty,
}

/// The two wire shapes of Table 5.2.8-1 `type`, before the non-empty rule is
/// checked on the array form.
#[derive(Deserialize)]
#[serde(untagged)]
enum EntityTypeSelectionWire {
    /// One name.
    Single(EntityTypeName),
    /// An array of names.
    Several(Vec<EntityTypeName>),
}

impl TryFrom<EntityTypeSelectionWire> for EntityTypeSelection {
    type Error = EntityTypeSelectionError;

    fn try_from(wire: EntityTypeSelectionWire) -> Result<EntityTypeSelection, EntityTypeSelectionError> {
        match wire {
            EntityTypeSelectionWire::Single(name) => Ok(EntityTypeSelection::Single(name)),
            EntityTypeSelectionWire::Several(names) if names.is_empty() => Err(EntityTypeSelectionError::Empty),
            EntityTypeSelectionWire::Several(names) => Ok(EntityTypeSelection::Several(names)),
        }
    }
}

/// What entities a context source can provide (Table 5.2.8-1): a type
/// selection, optionally narrowed to one identifier or to the identifiers a
/// pattern admits.
///
/// `type` has cardinality 1: a source cannot register entities by identifier
/// alone. `id` takes precedence over `idPattern` (clause 5.2.8), so an entry
/// carrying both is matched through its `id`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntityInfo {
    /// The entity type, or types, the source can provide.
    #[serde(rename = "type")]
    pub type_: EntityTypeSelection,
    /// The one entity identifier the source can provide.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Identifier>,
    /// The pattern the provided entity identifiers match.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id_pattern: Option<Pattern>,
}

impl EntityInfo {
    /// Whether the entry names its entity by `id` alone, with no `idPattern`
    /// beside it: the shape clause 4.3.6.3 asks of every entry of an
    /// exclusive registration, which relates to specific entities.
    #[must_use]
    pub const fn names_one_entity(&self) -> bool {
        self.id.is_some() && self.id_pattern.is_none()
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::entity_info::{EntityInfo, EntityTypeSelection};
    use serde_json::json;

    #[test]
    fn an_entity_info_type_round_trips_both_wire_shapes_as_written() {
        let single: EntityInfo = serde_json::from_value(json!({"type": "Vehicle"})).expect("string form parses");
        assert!(matches!(&single.type_, EntityTypeSelection::Single(name) if name.as_str() == "Vehicle"));
        pretty_assertions::assert_eq!(serde_json::to_value(&single).expect("serialises"), json!({"type": "Vehicle"}));

        let several: EntityInfo = serde_json::from_value(json!({"type": ["Vehicle", "Car"]})).expect("array form parses");
        assert!(matches!(&several.type_, EntityTypeSelection::Several(names) if names.len() == 2));
        pretty_assertions::assert_eq!(serde_json::to_value(&several).expect("serialises"), json!({"type": ["Vehicle", "Car"]}));

        let one: EntityInfo = serde_json::from_value(json!({"type": ["Vehicle"]})).expect("one-element array parses");
        pretty_assertions::assert_eq!(serde_json::to_value(&one).expect("serialises"), json!({"type": ["Vehicle"]}));
    }

    #[test]
    fn an_empty_type_array_a_non_string_type_and_a_missing_type_are_rejected() {
        assert!(serde_json::from_value::<EntityInfo>(json!({"type": []})).is_err());
        assert!(serde_json::from_value::<EntityInfo>(json!({"type": 5})).is_err());
        assert!(serde_json::from_value::<EntityInfo>(json!({"id": "urn:ngsi-ld:Vehicle:1"})).is_err());
    }

    #[test]
    fn a_selection_names_every_type_and_reads_as_their_alternation() {
        let several: EntityTypeSelection = serde_json::from_value(json!(["Vehicle", "Car"])).expect("parses");
        pretty_assertions::assert_eq!(several.names().len(), 2);
        pretty_assertions::assert_eq!(several.as_type_expression().to_string(), "Vehicle,Car");
        let single: EntityTypeSelection = serde_json::from_value(json!("Vehicle")).expect("parses");
        pretty_assertions::assert_eq!(single.as_type_expression().to_string(), "Vehicle");
    }

    #[test]
    fn only_an_entry_naming_an_id_without_a_pattern_names_one_entity() {
        let entry = |wire| serde_json::from_value::<EntityInfo>(wire).expect("a readable entry");
        assert!(entry(json!({"type": "Vehicle", "id": "urn:ngsi-ld:Vehicle:1"})).names_one_entity());
        assert!(!entry(json!({"type": "Vehicle", "id": "urn:ngsi-ld:Vehicle:1", "idPattern": "^urn:ngsi-ld:Vehicle:"})).names_one_entity());
        assert!(!entry(json!({"type": "Vehicle", "idPattern": "^urn:ngsi-ld:Vehicle:"})).names_one_entity());
        assert!(!entry(json!({"type": "Vehicle"})).names_one_entity());
    }
}
