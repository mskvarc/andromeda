//! The clause 4.21 list in force applied to one rendered temporal Linked
//! Entity (clauses 4.5.23, 5.7.1.5).
//!
//! Which list a Linked Entity is narrowed by is the Linked Entity engine's
//! decision, handed over as a [`MemberRule`]; this writes a copy of the entity
//! holding the members the rule keeps. The graph the engine planned stays
//! untouched, since an inline join nests the same entity under several
//! Relationships, each narrowed by its own path's list.

use crate::temporal::temporal_evolutions::TemporalMember;
use andromeda_linked_retrieval::narrowing::member::MemberRule;
use serde_json::{Map, Value};

/// A copy of the rendered temporal entity `node` holding the members `rule`
/// keeps: the `@context` and the clause 4.8 system times always, `id`, `type`
/// and `scope` by the keyword rule of the list, and the Attributes the list
/// returns.
#[must_use]
pub fn narrowed_temporal_entity(node: &Value, rule: MemberRule<'_>) -> Value {
    let Some(members) = node.as_object() else {
        return node.clone(); // clone: the payload owns its copy of the shared node
    };
    let kept: Map<String, Value> = members
        .iter()
        .filter(|(key, _)| match TemporalMember::of(key) {
            TemporalMember::Structural => true,
            TemporalMember::Keyword(keyword) => rule.keyword(keyword).is_write(),
            TemporalMember::Attribute => rule.member(key).is_write(),
        })
        .map(|(key, value)| (key.clone(), value.clone())) // clone: the payload owns its copy of the shared node
        .collect();
    Value::Object(kept)
}

#[cfg(test)]
mod tests {
    use crate::temporal::linked_projection::narrowed_temporal_entity;
    use andromeda_linked_retrieval::narrowing::{list::ListInForce, member::MemberRule};
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use andromeda_ngsi_ld_query::projection::{attribute_iri_index::AttributeIriIndex, term::ProjectionTerm};
    use iri_rs::IriBuf;
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};

    fn iri(name: &str) -> IriBuf {
        IriBuf::new(format!("https://example.com/{name}")).expect("valid IRI")
    }

    fn index() -> AttributeIriIndex {
        let mut index = AttributeIriIndex::new();
        for key in ["category", "name"] {
            index.insert(key.parse::<AttributeName>().expect("valid attribute name"), iri(key));
        }
        index
    }

    fn leaf(name: &str) -> ProjectionTerm {
        ProjectionTerm::Attribute {
            iri: iri(name),
            linked: Vec::new(),
        }
    }

    fn slot() -> Value {
        json!({
            "id": "urn:ngsi-ld:Slot:1",
            "type": "Slot",
            "scope": "/Madrid",
            "@context": "https://example.com/context.jsonld",
            "modifiedAt": "2024-01-01T00:00:00Z",
            "category": [{"type": "Property", "value": "reserved", "observedAt": "2024-01-01T00:00:00Z"}],
            "name": [{"type": "Property", "value": "Bay 3", "observedAt": "2024-01-01T00:00:00Z"}]
        })
    }

    /// The member keys of `value`, sorted.
    fn keys(value: &Value) -> Vec<&str> {
        let mut keys: Vec<&str> = value.as_object().expect("an object").keys().map(String::as_str).collect();
        keys.sort_unstable();
        keys
    }

    /// Clause 4.21: a list without `id` drops the Linked Entity's identity,
    /// as it would a root's (D3).
    #[test]
    fn a_list_without_id_drops_the_linked_identity() {
        let index = index();
        let pick = [leaf("category")];
        let narrowed = narrowed_temporal_entity(&slot(), MemberRule::new(ListInForce::Pick(&pick), &index));
        assert_eq!(keys(&narrowed), vec!["@context", "category", "modifiedAt"]);
    }

    #[test]
    fn a_list_naming_the_keywords_keeps_them() {
        let index = index();
        let pick = [ProjectionTerm::Id, ProjectionTerm::Scope, leaf("name")];
        let narrowed = narrowed_temporal_entity(&slot(), MemberRule::new(ListInForce::Pick(&pick), &index));
        assert_eq!(keys(&narrowed), vec!["@context", "id", "modifiedAt", "name", "scope"]);
    }

    #[test]
    fn an_omit_removes_only_what_it_names() {
        let index = index();
        let omit = [leaf("name")];
        let narrowed = narrowed_temporal_entity(&slot(), MemberRule::new(ListInForce::Omit(&omit), &index));
        assert_eq!(keys(&narrowed), vec!["@context", "category", "id", "modifiedAt", "scope", "type"]);
    }

    #[test]
    fn an_unrestricted_list_keeps_the_whole_entity() {
        let index = index();
        assert_eq!(narrowed_temporal_entity(&slot(), MemberRule::new(ListInForce::Unrestricted, &index)), slot());
    }
}
