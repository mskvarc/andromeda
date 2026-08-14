//! Applying the clause 4.21 structural-member rule to a rendered value.
//!
//! `pick` and `omit` may name `id`, `type` and `scope` as keywords alongside
//! attributes (clause 4.21). Read literally, a non-empty `pick` keeps one
//! of these three members only when it is explicitly listed, and a non-empty
//! `omit` drops whichever of the three it lists. `@context` is a JSON-LD
//! transport concern, never a structural member, and this rule never touches
//! it.
//!
//! The rule is not specific to the non-temporal entity representation: a
//! compacted temporal representation (`andromeda-temporal`) carries the same
//! `id` / `type` members and is pruned by the same rule, once compaction has
//! turned its expanded `@id` / `@type` into them. The function is public so
//! that crate can reuse it rather than reimplementing the rule.

use andromeda_ngsi_ld_query::projection::{compiled::Projection, term::ProjectionTerm};
use serde_json::Value;

/// Keep or drop the `id`, `type` and `scope` keys of a rendered entity value
/// according to the clause 4.21 structural-member rule.
///
/// A non-object value (should not occur for an entity rendering) is left
/// untouched rather than treated as an error, since pruning has nothing to
/// remove from it.
pub fn prune_structural_members(value: &mut Value, projection: &Projection) {
    let Some(map) = value.as_object_mut() else {
        return;
    };

    if !projection.pick.is_empty() {
        if !projection.pick.contains(&ProjectionTerm::Id) {
            map.remove("id");
        }
        if !projection.pick.contains(&ProjectionTerm::Type) {
            map.remove("type");
        }
        if !projection.pick.contains(&ProjectionTerm::Scope) {
            map.remove("scope");
        }
    }

    if !projection.omit.is_empty() {
        if projection.omit.contains(&ProjectionTerm::Id) {
            map.remove("id");
        }
        if projection.omit.contains(&ProjectionTerm::Type) {
            map.remove("type");
        }
        if projection.omit.contains(&ProjectionTerm::Scope) {
            map.remove("scope");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iri_rs::IriBuf;
    use serde_json::json;

    fn iri(value: &str) -> IriBuf {
        IriBuf::new(value.to_string()).expect("valid IRI")
    }

    /// Clause 4.21, literal reading: a non-empty `pick` naming only `id`
    /// keeps `id` and drops every other structural member and attribute.
    #[test]
    fn pick_of_id_keeps_only_id() {
        let mut value = json!({"id": "urn:x", "type": "T", "scope": "/Madrid", "speed": {"type": "Property", "value": 1}});
        let projection = Projection {
            pick: vec![ProjectionTerm::Id],
            omit: Vec::new(),
        };

        prune_structural_members(&mut value, &projection);

        assert_eq!(value["id"], "urn:x");
        assert!(value.get("type").is_none());
        assert!(value.get("scope").is_none());
    }

    /// Clause 4.21: `omit` removes the structural members it lists.
    #[test]
    fn omit_of_scope_removes_scope() {
        let mut value = json!({"id": "urn:x", "type": "T", "scope": "/Madrid"});
        let projection = Projection {
            pick: Vec::new(),
            omit: vec![ProjectionTerm::Scope],
        };

        prune_structural_members(&mut value, &projection);

        assert_eq!(value["id"], "urn:x");
        assert_eq!(value["type"], "T");
        assert!(value.get("scope").is_none());
    }

    /// Clause 4.21: `omit` removes `type` when it is listed.
    #[test]
    fn omit_of_type_removes_type() {
        let mut value = json!({"id": "urn:x", "type": "T"});
        let projection = Projection {
            pick: Vec::new(),
            omit: vec![ProjectionTerm::Type],
        };

        prune_structural_members(&mut value, &projection);

        assert_eq!(value["id"], "urn:x");
        assert!(value.get("type").is_none());
    }

    /// Clause 4.21, literal reading: a non-empty `pick` that names only an
    /// attribute leaves `id` and `type` out, since neither keyword was listed.
    #[test]
    fn pick_of_an_attribute_leaves_id_and_type_out() {
        let mut value = json!({"id": "urn:x", "type": "T", "speed": {"type": "Property", "value": 1}});
        let projection = Projection {
            pick: vec![ProjectionTerm::Attribute {
                iri: iri("https://example.com/speed"),
                linked: Vec::new(),
            }],
            omit: Vec::new(),
        };

        prune_structural_members(&mut value, &projection);

        assert!(value.get("id").is_none());
        assert!(value.get("type").is_none());
        assert_eq!(value["speed"]["value"], 1);
    }

    /// An empty `pick` and empty `omit` select every member with no
    /// restriction, so the structural members are left untouched.
    #[test]
    fn an_empty_projection_leaves_structural_members_untouched() {
        let mut value = json!({"id": "urn:x", "type": "T", "scope": "/Madrid"});

        prune_structural_members(&mut value, &Projection::default());

        assert_eq!(value["id"], "urn:x");
        assert_eq!(value["type"], "T");
        assert_eq!(value["scope"], "/Madrid");
    }

    /// `@context` is a JSON-LD transport concern, never a structural member,
    /// and survives every combination of `pick`/`omit`.
    #[test]
    fn context_is_never_touched() {
        let mut value = json!({"id": "urn:x", "type": "T", "@context": "https://example.com/context.jsonld"});
        let projection = Projection {
            pick: vec![ProjectionTerm::Id],
            omit: Vec::new(),
        };

        prune_structural_members(&mut value, &projection);

        assert_eq!(value["@context"], "https://example.com/context.jsonld");
    }
}
