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

    /// Whether `name` is one of the selected types.
    #[must_use]
    pub fn admits(&self, name: &str) -> bool {
        self.names().iter().any(|selected| selected.as_str() == name)
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
    /// Whether the entry covers the concrete identifier `entity_id`.
    ///
    /// Clause 5.2.8: an `id` is decisive when present; otherwise an `idPattern`
    /// decides; an entry constraining neither covers every identifier.
    #[must_use]
    pub fn covers_id(&self, entity_id: &Identifier) -> bool {
        match (&self.id, &self.id_pattern) {
            (Some(id), _) => id == entity_id,
            (None, Some(pattern)) => pattern.is_match(entity_id.as_str()),
            (None, None) => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
    fn a_selection_admits_any_of_its_names() {
        let several: EntityTypeSelection = serde_json::from_value(json!(["Vehicle", "Car"])).expect("parses");
        assert!(several.admits("Vehicle"));
        assert!(several.admits("Car"));
        assert!(!several.admits("Building"));
        pretty_assertions::assert_eq!(several.names().len(), 2);
        pretty_assertions::assert_eq!(several.as_type_expression().to_string(), "Vehicle,Car");
        let single: EntityTypeSelection = serde_json::from_value(json!("Vehicle")).expect("parses");
        pretty_assertions::assert_eq!(single.as_type_expression().to_string(), "Vehicle");
    }

    /// Clause 5.2.8: `id` takes precedence over `idPattern`.
    #[test]
    fn an_entry_with_an_id_covers_that_id_only_even_beside_a_pattern() {
        let entry: EntityInfo =
            serde_json::from_value(json!({"type": "Vehicle", "id": "urn:ngsi-ld:Vehicle:1", "idPattern": "urn:ngsi-ld:Vehicle:.*"})).expect("parses");
        assert!(entry.covers_id(&"urn:ngsi-ld:Vehicle:1".parse().expect("valid URI")));
        assert!(!entry.covers_id(&"urn:ngsi-ld:Vehicle:2".parse().expect("valid URI")));
        let pattern_only: EntityInfo = serde_json::from_value(json!({"type": "Vehicle", "idPattern": "urn:ngsi-ld:Vehicle:.*"})).expect("parses");
        assert!(pattern_only.covers_id(&"urn:ngsi-ld:Vehicle:2".parse().expect("valid URI")));
        assert!(!pattern_only.covers_id(&"urn:ngsi-ld:Building:2".parse().expect("valid URI")));
        let unconstrained: EntityInfo = serde_json::from_value(json!({"type": "Vehicle"})).expect("parses");
        assert!(unconstrained.covers_id(&"urn:ngsi-ld:Building:2".parse().expect("valid URI")));
    }
}
