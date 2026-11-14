//! The entity selector of ETSI GS CIM 009 clause 5.2.33, which a
//! Subscription's and a Query's `entities` are arrays of, and the one reading
//! of which entities it selects.

use crate::{attribute::one_or_many::OneOrMany, query::non_empty_member::NonEmptyMemberError};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_query::{identifier_axis::IdentifierAxis, pattern::Pattern, type_selection::grammar::TypeExprRaw};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as DeserializeError};

/// One entity selector (ETSI GS CIM 009 clause 5.2.33).
///
/// Selects entities by type and optionally by id or id pattern. Table 5.2.33-1
/// lists every member the selector has, so a member outside them is refused.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EntitySelector {
    /// The entity identifiers the selector admits.
    ///
    /// Table 5.2.33-1 types `id` as "String or String[]" of "Valid URI(s)": one
    /// identifier is read from a String and written back as one, several from
    /// and to an array. An empty array names no entity and is refused, so a
    /// present `id` always lists at least one identifier.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_optional_ids",
        serialize_with = "serialize_optional_ids"
    )]
    pub id: Option<Vec<Identifier>>,
    /// Regular expression the entity id must match (clause 5.2.33).
    #[serde(default, skip_serializing_if = "Option::is_none")]
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

/// Whether the union of `selectors` selects the entity typed `types` and
/// identified by `id`.
///
/// One selector selecting the entity suffices, since each selector's members
/// hold together and the array is their union (clause 5.2.33). No selector at
/// all constrains nothing, as for a Subscription naming `watchedAttributes`
/// only (clause 5.8.1.4).
#[must_use]
pub fn selectors_select<S: AsRef<str>>(selectors: &[EntitySelector], types: &[S], id: &Identifier) -> bool {
    selectors.is_empty() || selectors.iter().any(|selector| selector.selects(types, id))
}

/// Write the String-or-String-array union of Table 5.2.33-1 the way the
/// shortest form states it: one identifier as a String, several as an array.
///
/// The function is public because it names the wire form of `id`: it is the
/// encoding the member has, and serde's `serialize_with` hands it the member
/// as the `Option` the field holds.
///
/// # Errors
///
/// Returns the serializer's own error when the member cannot be written.
pub fn serialize_optional_ids<S>(ids: &Option<Vec<Identifier>>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    match ids.as_deref() {
        Some([only]) => only.serialize(serializer),
        Some(ids) => ids.serialize(serializer),
        None => serializer.serialize_none(),
    }
}

/// Accept the String-or-String-array union defined by Table 5.2.33-1.
///
/// The function is public for the reason [`serialize_optional_ids`] is: it is
/// the encoding the member has.
///
/// # Errors
///
/// Returns the deserializer's refusal of a member that is neither a String nor
/// an array, an element's own refusal of a value that is no identifier, and a
/// custom error for an empty array.
pub fn deserialize_optional_ids<'de, D>(deserializer: D) -> Result<Option<Vec<Identifier>>, D::Error>
where
    D: Deserializer<'de>,
{
    let ids = Option::<OneOrMany<Identifier>>::deserialize(deserializer)?.map(OneOrMany::into_vec);
    match ids {
        Some(ids) if ids.is_empty() => Err(DeserializeError::custom(NonEmptyMemberError::EmptyArray)),
        other => Ok(other),
    }
}

#[cfg(test)]
mod tests {
    use crate::entity_selector::{EntitySelector, selectors_select};
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

    /// Table 5.2.33-1 restricts `id` to valid URIs, in either form, so an empty
    /// array names none, and lists every member a selector has.
    #[test]
    fn an_id_that_is_no_uri_and_an_unknown_member_are_refused() {
        for wire in [
            json!({"type": "Vehicle", "id": "not a uri"}),
            json!({"type": "Vehicle", "id": ["urn:ngsi-ld:Vehicle:1", "not a uri"]}),
            json!({"type": "Vehicle", "id": 5}),
            json!({"type": "Vehicle", "id": []}),
            json!({"type": "Vehicle", "ids": ["urn:ngsi-ld:Vehicle:1"]}),
        ] {
            assert!(serde_json::from_value::<EntitySelector>(wire.clone()).is_err(), "{wire}");
        }
    }

    /// Whether the selectors written as `wire` select the entity
    /// `urn:ngsi-ld:A:1` of type `A`.
    fn selects_the_entity(wire: serde_json::Value) -> bool {
        let selectors: Vec<EntitySelector> = serde_json::from_value(wire).expect("readable selectors");
        selectors_select(&selectors, &["A"], &id("urn:ngsi-ld:A:1"))
    }

    /// Clause 5.8.1.4: a Subscription naming `watchedAttributes` only has no
    /// selector, and so constrains no entity.
    #[test]
    fn no_selector_selects_every_entity() {
        assert!(selects_the_entity(json!([])));
    }

    /// Clause 5.2.33: a selector's `type` and `id` hold together.
    #[test]
    fn a_selector_selects_an_entity_its_type_and_its_id_both_name() {
        assert!(selects_the_entity(json!([{"type": "A", "id": "urn:ngsi-ld:A:1"}])));
        assert!(!selects_the_entity(json!([{"type": "A", "id": "urn:ngsi-ld:A:2"}])));
        assert!(!selects_the_entity(json!([{"type": "B", "id": "urn:ngsi-ld:A:1"}])));
    }

    /// Clause 5.2.33: the selectors are a union of selectors, each held
    /// whole, never the cross product of their members.
    #[test]
    fn selectors_select_through_any_one_of_them_and_never_through_a_pair_none_names() {
        assert!(selects_the_entity(json!([{"type": "A", "id": "urn:ngsi-ld:A:2"}, {"type": "A"}])));
        assert!(!selects_the_entity(
            json!([{"type": "B", "id": "urn:ngsi-ld:A:1"}, {"type": "A", "id": "urn:ngsi-ld:A:2"}])
        ));
    }

    /// Clause 5.2.8: an `id` takes precedence over an `idPattern` beside it,
    /// even when the pattern would match.
    #[test]
    fn an_id_beside_a_matching_id_pattern_decides_alone() {
        assert!(!selects_the_entity(
            json!([{"type": "A", "id": "urn:ngsi-ld:A:2", "idPattern": "urn:ngsi-ld:A:.*"}])
        ));
    }

    /// Table 5.2.33-1: an `id` array selects any entity it lists.
    #[test]
    fn an_id_array_selects_a_listed_entity() {
        assert!(selects_the_entity(json!([{"type": "A", "id": ["urn:ngsi-ld:A:2", "urn:ngsi-ld:A:1"]}])));
    }

    /// Clause 4.17: a conjunction needs every type it names, and the wildcard
    /// selects an entity of any type.
    #[test]
    fn a_type_conjunction_needs_every_type_and_the_wildcard_takes_any() {
        assert!(!selects_the_entity(json!([{"type": "(A;B)"}])));
        assert!(selects_the_entity(json!([{"type": "*"}])));
    }
}
