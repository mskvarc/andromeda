//! The clause 4.21 nested projection applied to a Linked Entity resolved for
//! the temporal API (clause 4.5.23).
//!
//! `pick=rel{a}` says two things: keep `rel` on the root -- settled by
//! [`crate::temporal::entity`] before any Linked Entity is even resolved --
//! and narrow whatever `rel` links to down to `a` plus its identity. This is
//! the second half.
//!
//! Unlike the non-temporal `join=inline` nesting in [`crate::nested_projection`],
//! the temporal join in `andromeda-temporal`'s `crate::linked` resolves each
//! Linked Entity exactly once regardless of how many roots, attributes or
//! instances named it, so there is exactly one narrowing to compute and apply
//! per identifier -- the clause 4.21 union rule the non-temporal API's
//! `join=flat` applies, and the only rule the temporal API needs.

use andromeda_ngsi_ld_query::projection::{attribute_iri_index::AttributeIriIndex, compiled::Projection, matching::matching_term, term::ProjectionTerm};
use serde_json::Value;

/// The members of a rendered temporal Linked Entity a nested projection never
/// removes.
///
/// A temporal representation carries no member outside its attribute map
/// except these three -- the clause 4.8 system timestamps and the clause 4.7
/// geo-properties are themselves attribute-shaped rows in a temporal
/// representation, not separate members, so they are narrowed like any other
/// attribute rather than preserved unconditionally.
const PRESERVED_MEMBERS: [&str; 3] = ["id", "type", "@context"];

/// Narrow a resolved Linked Entity's temporal representation to the nested
/// `pick` / `omit` sub-projection that applies to it.
///
/// `scope` is governed by the clause 4.21 structural keyword, not by an
/// attribute term: a non-empty nested `pick` keeps it only when the nested
/// terms name the `scope` keyword, and a non-empty nested `omit` drops it
/// when they do.
pub fn narrow_linked_temporal_entity(value: &mut Value, projection: &Projection, index: &AttributeIriIndex) {
    let Some(map) = value.as_object_mut() else {
        return;
    };
    let keeps_scope = keeps_scope(projection);

    if !projection.pick.is_empty() {
        map.retain(|key, _| {
            if key == ProjectionTerm::SCOPE_KEYWORD {
                return keeps_scope;
            }
            PRESERVED_MEMBERS.contains(&key.as_str()) || matching_term(key, &projection.pick, index).is_some()
        });
    }

    if !projection.omit.is_empty() {
        map.retain(|key, _| {
            if key == ProjectionTerm::SCOPE_KEYWORD {
                return keeps_scope;
            }
            PRESERVED_MEMBERS.contains(&key.as_str())
                || match matching_term(key, &projection.omit, index) {
                    // Clause 4.21: `omit=rel{a}` omits `a` inside whatever
                    // `rel` links to, so `rel` itself survives and the
                    // omission travels one level further down.
                    Some(term) => !term.linked_terms().is_empty(),
                    None => true,
                }
        });
    }
}

/// Whether `scope` survives a nested projection, following the same clause
/// 4.21 structural rule the root rendering and the non-temporal API's
/// nested projection both apply.
fn keeps_scope(projection: &Projection) -> bool {
    if !projection.pick.is_empty() && !projection.pick.contains(&ProjectionTerm::Scope) {
        return false;
    }
    projection.omit.is_empty() || !projection.omit.contains(&ProjectionTerm::Scope)
}

#[cfg(test)]
mod tests {
    use super::*;
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use iri_rs::IriBuf;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    fn iri(name: &str) -> IriBuf {
        IriBuf::new(format!("https://example.com/{name}")).expect("valid IRI")
    }

    fn attribute_name(value: &str) -> AttributeName {
        value.parse().expect("valid attribute name")
    }

    fn index(keys: &[&str]) -> AttributeIriIndex {
        let mut index = AttributeIriIndex::new();
        for key in keys {
            index.insert(attribute_name(key), iri(key));
        }
        index
    }

    fn leaf(name: &str) -> ProjectionTerm {
        ProjectionTerm::Attribute {
            iri: iri(name),
            linked: Vec::new(),
        }
    }

    /// Clause 4.21: a nested `pick` reduces a resolved Linked Entity's
    /// temporal representation to the named attribute plus its identity.
    #[test]
    fn a_nested_pick_reduces_the_linked_entity_to_the_named_attribute() {
        let projection = Projection {
            pick: vec![leaf("category")],
            omit: Vec::new(),
        };
        let index = index(&["category", "name"]);
        let mut value = json!({
            "id": "urn:ngsi-ld:Slot:1",
            "type": "Slot",
            "category": [{"type": "Property", "value": "reserved", "observedAt": "2024-01-01T00:00:00Z"}],
            "name": [{"type": "Property", "value": "Bay 3", "observedAt": "2024-01-01T00:00:00Z"}]
        });

        narrow_linked_temporal_entity(&mut value, &projection, &index);

        assert_eq!(value["id"], "urn:ngsi-ld:Slot:1");
        assert_eq!(value["type"], "Slot");
        assert!(value.get("category").is_some());
        assert!(value.get("name").is_none());
    }

    /// A nested `omit` removes only the named attribute.
    #[test]
    fn a_nested_omit_removes_only_the_named_attribute() {
        let projection = Projection {
            pick: Vec::new(),
            omit: vec![leaf("name")],
        };
        let index = index(&["category", "name"]);
        let mut value = json!({
            "id": "urn:ngsi-ld:Slot:1",
            "type": "Slot",
            "category": [{"type": "Property", "value": "reserved"}],
            "name": [{"type": "Property", "value": "Bay 3"}]
        });

        narrow_linked_temporal_entity(&mut value, &projection, &index);

        assert!(value.get("category").is_some());
        assert!(value.get("name").is_none());
    }

    /// A nested `pick` that does not name the `scope` keyword drops `scope`.
    #[test]
    fn a_nested_pick_drops_scope_unless_the_scope_keyword_is_nested_too() {
        let index = index(&["category"]);
        let value = || {
            json!({
                "id": "urn:ngsi-ld:Slot:1",
                "type": "Slot",
                "scope": "/Madrid",
                "category": [{"type": "Property", "value": "reserved"}]
            })
        };

        let without_keyword = Projection {
            pick: vec![leaf("category")],
            omit: Vec::new(),
        };
        let mut narrowed = value();
        narrow_linked_temporal_entity(&mut narrowed, &without_keyword, &index);
        assert!(narrowed.get("scope").is_none());

        let with_keyword = Projection {
            pick: vec![leaf("category"), ProjectionTerm::Scope],
            omit: Vec::new(),
        };
        let mut kept = value();
        narrow_linked_temporal_entity(&mut kept, &with_keyword, &index);
        assert_eq!(kept["scope"], "/Madrid");
    }

    /// Identity and `@context` survive every nested projection.
    #[test]
    fn identity_and_context_survive_every_nested_projection() {
        let projection = Projection {
            pick: vec![leaf("category")],
            omit: Vec::new(),
        };
        let index = index(&["category"]);
        let mut value = json!({
            "id": "urn:ngsi-ld:Slot:1",
            "type": "Slot",
            "@context": "https://example.com/context.jsonld",
            "category": [{"type": "Property", "value": "reserved"}]
        });

        narrow_linked_temporal_entity(&mut value, &projection, &index);

        assert_eq!(value["id"], "urn:ngsi-ld:Slot:1");
        assert_eq!(value["type"], "Slot");
        assert_eq!(value["@context"], "https://example.com/context.jsonld");
    }
}
