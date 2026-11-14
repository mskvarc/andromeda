//! The entity selector of a subscription (ETSI GS CIM 009 clause 5.2.33) and
//! the one reading of which entities it selects.

use crate::query::entity_selector::{deserialize_optional_ids, serialize_optional_ids};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_query::{identifier_axis::IdentifierAxis, pattern::Pattern, type_selection::grammar::TypeExprRaw};
use serde::{Deserialize, Serialize};

/// Entity selector for subscriptions (ETSI GS CIM 009 clause 5.2.33).
///
/// Selects entities by type and optionally by id or id pattern. Table 5.2.33-1
/// lists every member the selector has, so a member outside them is refused.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EntitySelector {
    /// The entity identifiers the selector admits.
    ///
    /// Table 5.2.33-1 types `id` as "String or String[]": one identifier is
    /// read from a String and written back as one, several from and to an
    /// array.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_optional_ids",
        serialize_with = "serialize_optional_ids"
    )]
    pub id: Option<Vec<Identifier>>,
    /// Regular expression the entity id must match (clause 5.2.33).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id_pattern: Option<Pattern>,
    /// Type selection expression the entity's types must satisfy (required).
    ///
    /// A clause 4.17 expression rather than a single name: a selector may
    /// alternate, as in `(Building|Tower)`. Matching is by whole type name:
    /// `Building` does not match an entity typed `MyBuilding`.
    #[serde(rename = "type")]
    pub type_: TypeExprRaw,
}

impl EntitySelector {
    /// What this selector constrains about the entity identifier, `id` taking
    /// precedence over `idPattern` (clauses 5.2.8 and 5.2.33).
    #[must_use]
    pub fn identifier_axis(&self) -> IdentifierAxis<'_> {
        IdentifierAxis::of(self.id.as_deref().unwrap_or_default(), self.id_pattern.as_ref())
    }

    /// Whether this selector selects the entity typed `types` and identified
    /// by `id`: the types satisfy the clause 4.17 expression and the
    /// identifier its identifier axis.
    #[must_use]
    pub fn selects<S: AsRef<str>>(&self, types: &[S], id: &Identifier) -> bool {
        self.type_.matches(types) && self.identifier_axis().admits(id)
    }
}

#[cfg(test)]
mod tests {
    use crate::subscription::entity_selector::EntitySelector;
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_ngsi_ld_query::type_selection::grammar::TypeExprRaw;
    use serde_json::json;

    fn selector(wire: serde_json::Value) -> EntitySelector {
        serde_json::from_value(wire).expect("a readable selector")
    }

    fn id(value: &str) -> Identifier {
        value.parse().expect("valid identifier")
    }

    /// Table 5.2.33-1: an `EntitySelector` `type` is a clause 4.17 selection
    /// string; the array form belongs to `EntityInfo` (Table 5.2.8-1) alone.
    #[test]
    fn an_entity_selector_type_is_a_selection_string() {
        assert!(matches!(selector(json!({"type": "Vehicle"})).type_, TypeExprRaw::Type(_)));
        assert!(matches!(&selector(json!({"type": "Vehicle|Car"})).type_, TypeExprRaw::Or(alternatives) if alternatives.len() == 2));
        assert!(serde_json::from_value::<EntitySelector>(json!({"type": ["Vehicle", "Car"]})).is_err());
        assert!(serde_json::from_value::<EntitySelector>(json!({"type": 5})).is_err());
    }

    /// Clause 4.17 decides the types, clause 5.2.33 the identifier, with `id`
    /// voiding an `idPattern` written beside it (clause 5.2.8).
    #[test]
    fn a_selector_selects_by_type_expression_and_identifier_axis() {
        let alternation = selector(json!({"type": "Vehicle|Car"}));
        assert!(alternation.selects(&["Car"], &id("urn:ngsi-ld:Car:1")));
        assert!(!alternation.selects(&["Building"], &id("urn:ngsi-ld:Building:1")));

        let conjunction = selector(json!({"type": "(Vehicle;Electric)"}));
        assert!(conjunction.selects(&["Vehicle", "Electric"], &id("urn:ngsi-ld:Vehicle:1")));
        assert!(!conjunction.selects(&["Vehicle"], &id("urn:ngsi-ld:Vehicle:1")));

        let patterned = selector(json!({"type": "Vehicle", "idPattern": "^urn:ngsi-ld:Vehicle:A"}));
        assert!(patterned.selects(&["Vehicle"], &id("urn:ngsi-ld:Vehicle:A1")));
        assert!(!patterned.selects(&["Vehicle"], &id("urn:ngsi-ld:Vehicle:B1")));

        let named = selector(json!({"type": "Vehicle", "id": "urn:ngsi-ld:Vehicle:B1", "idPattern": "^urn:ngsi-ld:Vehicle:A"}));
        assert!(named.selects(&["Vehicle"], &id("urn:ngsi-ld:Vehicle:B1")));
        assert!(!named.selects(&["Vehicle"], &id("urn:ngsi-ld:Vehicle:A1")));
    }

    /// Table 5.2.33-1 types `id` as "String or String[]": an array selects
    /// each entity it lists, and still voids an `idPattern` beside it.
    #[test]
    fn an_id_array_selects_each_listed_entity_and_voids_an_id_pattern() {
        let listed = selector(json!({
            "type": "Vehicle",
            "id": ["urn:ngsi-ld:Vehicle:2", "urn:ngsi-ld:Vehicle:1"],
            "idPattern": "^urn:ngsi-ld:Vehicle:3"
        }));
        assert!(listed.selects(&["Vehicle"], &id("urn:ngsi-ld:Vehicle:1")));
        assert!(listed.selects(&["Vehicle"], &id("urn:ngsi-ld:Vehicle:2")));
        assert!(!listed.selects(&["Vehicle"], &id("urn:ngsi-ld:Vehicle:3")));
        assert!(!listed.selects(&["Building"], &id("urn:ngsi-ld:Vehicle:1")));
    }

    /// One identifier reads back as the String Table 5.2.33-1 allows, several
    /// as the array they were written as, in their order.
    #[test]
    fn an_id_writes_back_as_a_string_for_one_identifier_and_an_array_for_several() {
        let one = selector(json!({"type": "Vehicle", "id": ["urn:ngsi-ld:Vehicle:1"]}));
        pretty_assertions::assert_eq!(
            serde_json::to_value(&one).expect("serialisable"),
            json!({"type": "Vehicle", "id": "urn:ngsi-ld:Vehicle:1"})
        );
        let several = json!({"type": "Vehicle", "id": ["urn:ngsi-ld:Vehicle:2", "urn:ngsi-ld:Vehicle:1"]});
        pretty_assertions::assert_eq!(serde_json::to_value(selector(several.clone())).expect("serialisable"), several);
    }

    /// Table 5.2.33-1 restricts `id` to valid URIs, in either form, and lists
    /// every member a selector has.
    #[test]
    fn an_id_that_is_no_uri_and_an_unknown_member_are_refused() {
        for wire in [
            json!({"type": "Vehicle", "id": "not a uri"}),
            json!({"type": "Vehicle", "id": ["urn:ngsi-ld:Vehicle:1", "not a uri"]}),
            json!({"type": "Vehicle", "id": 5}),
            json!({"type": "Vehicle", "ids": ["urn:ngsi-ld:Vehicle:1"]}),
        ] {
            assert!(serde_json::from_value::<EntitySelector>(wire.clone()).is_err(), "{wire}");
        }
    }
}
