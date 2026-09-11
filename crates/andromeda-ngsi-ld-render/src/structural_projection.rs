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
//! turned its expanded `@id` / `@type` into them. The type is public so that
//! crate can reuse it rather than reimplementing the rule.

use andromeda_ngsi_ld_query::projection::{compiled::Projection, term::ProjectionTerm};
use serde_json::Value;

/// The JSON-LD keyword carrying the active context.
///
/// A JSON-LD keyword rather than an NGSI-LD member, so no vocabulary constant
/// names it and none should.
const CONTEXT_KEYWORD: &str = "@context";

/// The clause 4.21 `pick` / `omit` terms that decide a rendered value's
/// structural members.
///
/// Borrowed term lists rather than a [`Projection`] because the rule applies at
/// two depths: to a response's own entities, where the lists are the request's
/// own, and to a Linked Entity, where they are the nested `{...}` sub-terms of
/// the Relationship that reached it (clause 4.21, clause 5.7.1.5 "applies to
/// all Entities in the payload in the case of Linked Entity retrieval"). A
/// nested level holds slices of the request's projection tree and owns no
/// `Projection` of its own.
#[derive(Clone, Copy, Debug)]
pub struct StructuralProjection<'a> {
    /// The `pick` terms in force at this depth.
    pick: &'a [ProjectionTerm],
    /// The `omit` terms in force at this depth.
    omit: &'a [ProjectionTerm],
}

impl<'a> StructuralProjection<'a> {
    /// The rule stated by one pair of term lists.
    #[must_use]
    pub const fn new(pick: &'a [ProjectionTerm], omit: &'a [ProjectionTerm]) -> StructuralProjection<'a> {
        StructuralProjection { pick, omit }
    }

    /// Keep or drop the `id`, `type` and `scope` keys of a rendered entity
    /// value according to the clause 4.21 structural-member rule.
    ///
    /// A non-object value (should not occur for an entity rendering) is left
    /// untouched rather than treated as an error, since pruning has nothing to
    /// remove from it.
    pub fn prune(self, value: &mut Value) {
        let Some(map) = value.as_object_mut() else {
            return;
        };
        for (keyword, key) in [
            (ProjectionTerm::Id, ProjectionTerm::ID_KEYWORD),
            (ProjectionTerm::Type, ProjectionTerm::TYPE_KEYWORD),
            (ProjectionTerm::Scope, ProjectionTerm::SCOPE_KEYWORD),
        ] {
            if !self.keeps(&keyword) {
                map.remove(key);
            }
        }
    }

    /// Whether a structural member survives: a non-empty `pick` keeps it only
    /// when it is listed, and an `omit` drops it when it is.
    #[must_use]
    pub fn keeps(self, keyword: &ProjectionTerm) -> bool {
        if !self.pick.is_empty() && !self.pick.contains(keyword) {
            return false;
        }
        !self.omit.contains(keyword)
    }
}

// A request's own projection states the rule for the entities it selected.
impl<'a> From<&'a Projection> for StructuralProjection<'a> {
    fn from(projection: &'a Projection) -> StructuralProjection<'a> {
        StructuralProjection::new(&projection.pick, &projection.omit)
    }
}

/// Whether a rendered entity value still carries an Entity member.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RenderedEntityMembers {
    /// At least one Entity member is written, so there is an entity to answer
    /// with.
    Present,
    /// Nothing but JSON-LD transport is left.
    None,
}

/// What a clause 4.21 projection left of a rendered entity value.
///
/// Clause 5.7.1.5 reduces an Entity "down to only contain the defined Entity
/// members"; when the projection defines none of the members the Entity has,
/// what is left is no Entity at all, and clause 5.7.1.4 answers a retrieval of
/// it with `ResourceNotFound`. `@context` is a JSON-LD transport concern rather
/// than an Entity member (clause 5.2.4), so it does not keep an entity alive.
///
/// A value that is no object carries no member either, which is the same
/// answer.
#[must_use]
pub fn rendered_entity_members(value: &Value) -> RenderedEntityMembers {
    let carries_member = value.as_object().is_some_and(|map| map.keys().any(|key| key != CONTEXT_KEYWORD));
    if carries_member {
        RenderedEntityMembers::Present
    } else {
        RenderedEntityMembers::None
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

        StructuralProjection::from(&projection).prune(&mut value);

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

        StructuralProjection::from(&projection).prune(&mut value);

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

        StructuralProjection::from(&projection).prune(&mut value);

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

        StructuralProjection::from(&projection).prune(&mut value);

        assert!(value.get("id").is_none());
        assert!(value.get("type").is_none());
        assert_eq!(value["speed"]["value"], 1);
    }

    /// An empty `pick` and empty `omit` select every member with no
    /// restriction, so the structural members are left untouched.
    #[test]
    fn an_empty_projection_leaves_structural_members_untouched() {
        let mut value = json!({"id": "urn:x", "type": "T", "scope": "/Madrid"});

        StructuralProjection::from(&Projection::default()).prune(&mut value);

        assert_eq!(value["id"], "urn:x");
        assert_eq!(value["type"], "T");
        assert_eq!(value["scope"], "/Madrid");
    }

    /// Clause 5.7.1.5: a value reduced to JSON-LD transport alone is no Entity.
    #[test]
    fn a_value_left_with_only_a_context_carries_no_entity_member() {
        assert_eq!(
            rendered_entity_members(&json!({"@context": "https://example.com/c.jsonld"})),
            RenderedEntityMembers::None
        );
        assert_eq!(rendered_entity_members(&json!({})), RenderedEntityMembers::None);
        assert_eq!(rendered_entity_members(&Value::Null), RenderedEntityMembers::None);
    }

    /// One surviving member, structural or attribute, is an Entity to answer
    /// with.
    #[test]
    fn a_value_left_with_one_member_carries_an_entity_member() {
        assert_eq!(rendered_entity_members(&json!({"id": "urn:x"})), RenderedEntityMembers::Present);
        assert_eq!(
            rendered_entity_members(&json!({"@context": "https://example.com/c.jsonld", "speed": {"type": "Property", "value": 1}})),
            RenderedEntityMembers::Present
        );
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

        StructuralProjection::from(&projection).prune(&mut value);

        assert_eq!(value["@context"], "https://example.com/context.jsonld");
    }
}
