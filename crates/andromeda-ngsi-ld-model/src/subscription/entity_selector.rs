//! The entity selector of a subscription (ETSI GS CIM 009 clause 5.2.33) and
//! the one reading of which entities it selects.

use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_query::{identifier_axis::IdentifierAxis, pattern::Pattern, type_selection::grammar::TypeExprRaw};
use serde::{Deserialize, Serialize};

/// Entity selector for subscriptions (ETSI GS CIM 009 clause 5.2.33).
///
/// Selects entities by type and optionally by id or id pattern.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntitySelector {
    /// Exact entity id to match.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Identifier>,
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
    pub const fn identifier_axis(&self) -> IdentifierAxis<'_> {
        IdentifierAxis::of(self.id.as_slice(), self.id_pattern.as_ref())
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
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
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
}
