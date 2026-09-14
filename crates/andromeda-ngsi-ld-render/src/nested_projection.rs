//! The clause 4.21 nested projection a `join=inline` rendering applies to the
//! linked entities it nests.
//!
//! `pick=rel{a,b}` says two things at once: keep `rel` on the entity that
//! carries it, and render whatever `rel` links to reduced to `a` and `b`. The
//! first half is settled before rendering, on the typed entity. The second
//! half belongs here, because `join=inline` nests a *separate copy* of a
//! linked entity under every Relationship that names it: `pick=relA{a},relB`
//! must show the target narrowed under `relA` and in full under `relB`, which
//! only the code that walks the Relationships one instance at a time can do.
//!
//! A linked entity is narrowed by exactly the rule its Relationship's nested
//! terms state, structural members included. Clause 5.7.1.5 says so in one
//! parenthesis -- a restrictive list reduces the Entity "down to only contain
//! the defined Entity members (applies to all Entities in the payload in the
//! case of Linked Entity retrieval)" -- and clause 4.21 gives the projection
//! language the same reach: it specifies "the Attributes to be retrieved within
//! an Entity (or associated Linked Entities when Linked Entity Retrieval is
//! used)". So `pick=locatedAt{id,name}` leaves the linked entity with `id` and
//! `name`, and with no `type`, because the nested terms did not name it.

use crate::structural_projection::StructuralProjection;
use andromeda_ngsi_ld_query::projection::{attribute_iri_index::AttributeIriIndex, compiled::Projection, matching::matching_term, term::ProjectionTerm};
use andromeda_ngsi_ld_vocabulary::compact::properties as compact_properties;
use serde_json::Value;

/// The JSON-LD keyword carrying the active context.
///
/// A JSON-LD keyword rather than an NGSI-LD member, so no vocabulary constant
/// names it and none should.
const CONTEXT_KEYWORD: &str = "@context";

/// The members a nested projection never removes.
///
/// `@context` is a JSON-LD transport concern rather than an Entity member, and
/// the clause 4.8 system timestamps and the clause 4.22 expiry are governed by
/// the `sysAttrs` representation option instead -- whether they appear at all
/// is that option's decision (clause 4.8), which is orthogonal to the clause
/// 4.21 projection and must not be overridden by it. `join=flat` narrows a
/// linked entity by editing the typed entity rather than the rendered value and
/// reaches none of these either, which is what keeps the two join modes
/// returning the same linked entity in the same shape.
///
/// `id`, `type` and `scope` are deliberately absent: clause 4.21 names all
/// three by *keyword*, so [`StructuralProjection`] decides them rather than a
/// term match. The clause 4.7 geo-properties are absent too: they are ordinary
/// Attributes that the model happens to keep in Entity members of their own
/// (clause 5.2.4), and a projection selects them by name like any other.
const PRESERVED_MEMBERS: [&str; 5] = [
    CONTEXT_KEYWORD,
    compact_properties::CREATED_AT,
    compact_properties::MODIFIED_AT,
    compact_properties::DELETED_AT,
    compact_properties::EXPIRES_AT,
];

/// Whether `key` is one of the three members clause 4.21 names by keyword.
///
/// Those three are left standing by the attribute passes and settled
/// afterwards by [`StructuralProjection::prune`], because a term match can
/// never recognize them: they have no IRI to expand to.
fn is_structural_member(key: &str) -> bool {
    matches!(key, ProjectionTerm::ID_KEYWORD | ProjectionTerm::TYPE_KEYWORD | ProjectionTerm::SCOPE_KEYWORD)
}

/// The clause 4.21 nested projection a `join=inline` rendering carries.
///
/// Only the *nested* sub-terms of these `pick` / `omit` terms apply: the terms
/// themselves already selected the members of the entity being rendered.
#[derive(Debug, Clone, Copy)]
pub struct LinkedProjection<'a> {
    /// The request's compiled `pick` / `omit` terms.
    pub projection: &'a Projection,
    /// The expanded IRI of every attribute key the payload carries, so a term
    /// can be matched against a key without a JSON-LD engine.
    pub index: &'a AttributeIriIndex,
}

/// The sub-projection in force at one point of the inline expansion.
///
/// Copied rather than cloned as the walk descends: both term lists are borrowed
/// slices of the request's projection tree, so descending a level costs nothing.
#[derive(Debug, Clone, Copy)]
pub(crate) struct NestedProjection<'a> {
    /// Nested `pick` terms governing this level's linked entities.
    pick: &'a [ProjectionTerm],
    /// Nested `omit` terms governing this level's linked entities.
    omit: &'a [ProjectionTerm],
    /// The expansion index, absent when no projection is in force at all.
    index: Option<&'a AttributeIriIndex>,
}

impl NestedProjection<'_> {
    /// The projection that narrows nothing, for a rendering with no `pick` or
    /// `omit`.
    pub(crate) const NONE: NestedProjection<'static> = NestedProjection {
        pick: &[],
        omit: &[],
        index: None,
    };
}

impl<'a> NestedProjection<'a> {
    /// The projection in force at the top of an inline expansion.
    pub(crate) const fn root(linked: LinkedProjection<'a>) -> NestedProjection<'a> {
        NestedProjection {
            pick: linked.projection.pick.as_slice(),
            omit: linked.projection.omit.as_slice(),
            index: Some(linked.index),
        }
    }

    /// The projection governing whatever the attribute keyed `key` links to.
    ///
    /// An attribute no term names contributes no restriction, which is what
    /// leaves a Relationship reached without a nested sub-projection rendering
    /// its target in full.
    pub(crate) fn descend(&self, key: &str) -> NestedProjection<'a> {
        let Some(index) = self.index else {
            return NestedProjection::NONE;
        };
        NestedProjection {
            pick: matching_term(key, self.pick, index).map_or(&[], ProjectionTerm::linked_terms),
            omit: matching_term(key, self.omit, index).map_or(&[], ProjectionTerm::linked_terms),
            index: Some(index),
        }
    }

    /// Whether this projection removes nothing.
    pub(crate) const fn is_unrestricted(&self) -> bool {
        self.pick.is_empty() && self.omit.is_empty()
    }

    /// The clause 4.21 keyword rule this projection states for `id`, `type`
    /// and `scope`.
    ///
    /// The same rule `join=flat` applies to its own linked entities, so both
    /// join modes narrow the structural members identically.
    const fn structural(&self) -> StructuralProjection<'a> {
        StructuralProjection::new(self.pick, self.omit)
    }

    /// Reduce a rendered linked entity to the members this projection keeps.
    pub(crate) fn narrow(&self, value: &mut Value) {
        let Some(index) = self.index else {
            return;
        };

        if let Some(map) = value.as_object_mut() {
            if !self.pick.is_empty() {
                map.retain(|key, _| is_structural_member(key) || PRESERVED_MEMBERS.contains(&key.as_str()) || matching_term(key, self.pick, index).is_some());
            }

            if !self.omit.is_empty() {
                map.retain(|key, _| {
                    is_structural_member(key)
                        || PRESERVED_MEMBERS.contains(&key.as_str())
                        || match matching_term(key, self.omit, index) {
                            // Clause 4.21: `omit=rel{a}` omits `a` inside whatever
                            // `rel` links to, so `rel` itself survives and the
                            // omission travels one level further down.
                            Some(term) => !term.linked_terms().is_empty(),
                            None => true,
                        }
                });
            }
        }

        self.structural().prune(value);
    }
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

    fn nested(name: &str, linked: Vec<ProjectionTerm>) -> ProjectionTerm {
        ProjectionTerm::Attribute { iri: iri(name), linked }
    }

    /// Clause 4.21: descending through `rel` yields the sub-projection `rel`
    /// carried, which then narrows the entity `rel` links to.
    #[test]
    fn descending_through_a_relationship_narrows_its_target_to_the_nested_terms() {
        let projection = Projection {
            pick: vec![nested("rel", vec![leaf("category")])],
            omit: Vec::new(),
        };
        let index = index(&["rel", "category", "name"]);
        let root = NestedProjection::root(LinkedProjection {
            projection: &projection,
            index: &index,
        });

        let target = root.descend("rel");
        let mut value = json!({"id": "urn:b", "type": "B", "category": {"type": "Property", "value": "x"}, "name": {"type": "Property", "value": "y"}});
        target.narrow(&mut value);

        // Clause 5.7.1.5 reduces the linked entity "down to only contain the
        // defined Entity members", and the nested terms defined one.
        assert!(value.get("id").is_none());
        assert!(value.get("type").is_none());
        assert_eq!(value["category"]["value"], "x");
        assert!(value.get("name").is_none());
    }

    /// Clause 4.21, per-instance semantics: a second Relationship that carries
    /// no sub-projection leaves its own copy of the same target in full.
    #[test]
    fn a_relationship_without_a_sub_projection_narrows_nothing() {
        let projection = Projection {
            pick: vec![nested("relA", vec![leaf("category")]), leaf("relB")],
            omit: Vec::new(),
        };
        let index = index(&["relA", "relB", "category", "name"]);
        let root = NestedProjection::root(LinkedProjection {
            projection: &projection,
            index: &index,
        });

        let target = root.descend("relB");
        assert!(target.is_unrestricted());

        let mut value = json!({"id": "urn:b", "type": "B", "category": {"type": "Property", "value": "x"}, "name": {"type": "Property", "value": "y"}});
        target.narrow(&mut value);
        assert!(value.get("name").is_some());
    }

    /// Clause 4.21: a nested `omit` removes the named member from the linked
    /// entity and leaves the rest, identity included.
    #[test]
    fn a_nested_omit_removes_only_the_named_member() {
        let projection = Projection {
            pick: Vec::new(),
            omit: vec![nested("rel", vec![leaf("name")])],
        };
        let index = index(&["rel", "category", "name"]);
        let root = NestedProjection::root(LinkedProjection {
            projection: &projection,
            index: &index,
        });

        let mut value = json!({"id": "urn:b", "type": "B", "category": {"type": "Property", "value": "x"}, "name": {"type": "Property", "value": "y"}});
        root.descend("rel").narrow(&mut value);

        assert_eq!(value["id"], "urn:b");
        assert!(value.get("category").is_some());
        assert!(value.get("name").is_none());
    }

    /// Clause 4.21: `omit=rel{a}` omits `a` inside what `rel` links to, so a
    /// nested `omit` term that itself carries a sub-projection leaves the
    /// Relationship in place and travels one level further down.
    #[test]
    fn an_omit_term_carrying_a_sub_projection_keeps_the_relationship() {
        let projection = Projection {
            pick: Vec::new(),
            omit: vec![nested("outer", vec![nested("inner", vec![leaf("name")])])],
        };
        let index = index(&["outer", "inner", "name"]);
        let root = NestedProjection::root(LinkedProjection {
            projection: &projection,
            index: &index,
        });

        let level_one = root.descend("outer");
        let mut value = json!({"id": "urn:b", "type": "B", "inner": {"type": "Relationship", "object": "urn:c"}});
        level_one.narrow(&mut value);
        assert!(value.get("inner").is_some());

        let mut deeper = json!({"id": "urn:c", "type": "C", "name": {"type": "Property", "value": "y"}});
        level_one.descend("inner").narrow(&mut deeper);
        assert!(deeper.get("name").is_none());
    }

    /// Clause 4.21 reduces a linked entity to the named attributes plus `id`
    /// and `type`. `scope` is not on that list, so a nested `pick` that does
    /// not name the `scope` keyword drops it — the same clause 4.21
    /// structural rule `join=flat` applies through
    /// `apply_structural_scope_projection`. The counterpart flat-side test is
    /// `andromeda-entity`'s
    /// `the_flat_narrowing_leaves_the_same_linked_entity_shape_as_the_inline_one`;
    /// the two must agree member for member.
    #[test]
    fn a_nested_pick_drops_scope_unless_the_scope_keyword_is_nested_too() {
        let index = index(&["rel", "category"]);
        let value = || json!({"id": "urn:b", "type": "B", "scope": "/Madrid", "category": {"type": "Property", "value": "x"}});

        let without_keyword = Projection {
            pick: vec![nested("rel", vec![leaf("category")])],
            omit: Vec::new(),
        };
        let mut narrowed = value();
        NestedProjection::root(LinkedProjection {
            projection: &without_keyword,
            index: &index,
        })
        .descend("rel")
        .narrow(&mut narrowed);
        assert!(narrowed.get("scope").is_none());

        let with_keyword = Projection {
            pick: vec![nested("rel", vec![leaf("category"), ProjectionTerm::Scope])],
            omit: Vec::new(),
        };
        let mut kept = value();
        NestedProjection::root(LinkedProjection {
            projection: &with_keyword,
            index: &index,
        })
        .descend("rel")
        .narrow(&mut kept);
        assert_eq!(kept["scope"], "/Madrid");
    }

    /// Clause 4.21: a nested `omit` naming the `scope` keyword drops
    /// `scope`, and one that does not leaves it.
    #[test]
    fn a_nested_omit_drops_scope_only_when_it_names_the_scope_keyword() {
        let index = index(&["rel", "name"]);
        let value = || json!({"id": "urn:b", "type": "B", "scope": "/Madrid", "name": {"type": "Property", "value": "y"}});

        let naming_scope = Projection {
            pick: Vec::new(),
            omit: vec![nested("rel", vec![ProjectionTerm::Scope])],
        };
        let mut dropped = value();
        NestedProjection::root(LinkedProjection {
            projection: &naming_scope,
            index: &index,
        })
        .descend("rel")
        .narrow(&mut dropped);
        assert!(dropped.get("scope").is_none());

        let naming_an_attribute = Projection {
            pick: Vec::new(),
            omit: vec![nested("rel", vec![leaf("name")])],
        };
        let mut kept = value();
        NestedProjection::root(LinkedProjection {
            projection: &naming_an_attribute,
            index: &index,
        })
        .descend("rel")
        .narrow(&mut kept);
        assert_eq!(kept["scope"], "/Madrid");
    }

    /// What a nested projection may and may not reach.
    ///
    /// `@context` is JSON-LD transport, and whether the clause 4.8 timestamps
    /// and the clause 4.22 expiry appear is the `sysAttrs` representation
    /// option's decision — orthogonal to the clause 4.21 projection, which must
    /// not override it. Everything else the entity carries is a member clause
    /// 5.7.1.5 reduces away unless the nested terms defined it, the three
    /// clause 5.2.4 geo-properties included. The `join=flat` counterpart is
    /// `andromeda-entity`'s
    /// `the_flat_narrowing_leaves_the_same_linked_entity_shape_as_the_inline_one`,
    /// and the two must agree member for member or one request would answer
    /// two ways.
    #[test]
    fn a_nested_pick_keeps_only_the_members_the_nested_terms_defined() {
        let projection = Projection {
            pick: vec![nested("rel", vec![leaf("category")])],
            omit: Vec::new(),
        };
        let index = index(&["rel", "category", "name"]);
        let mut value = json!({
            "id": "urn:b",
            "type": "B",
            "@context": "https://example.com/context.jsonld",
            "createdAt": "2026-01-01T00:00:00Z",
            "modifiedAt": "2026-01-02T00:00:00Z",
            "deletedAt": "2026-01-03T00:00:00Z",
            "expiresAt": "2026-02-01T00:00:00Z",
            "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [0.0, 0.0]}},
            "observationSpace": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [1.0, 1.0]}},
            "operationSpace": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [2.0, 2.0]}},
            "category": {"type": "Property", "value": "x"},
            "name": {"type": "Property", "value": "y"}
        });

        NestedProjection::root(LinkedProjection {
            projection: &projection,
            index: &index,
        })
        .descend("rel")
        .narrow(&mut value);

        for member in ["@context", "createdAt", "modifiedAt", "deletedAt", "expiresAt", "category"] {
            assert!(value.get(member).is_some(), "{member} must survive a nested projection");
        }
        // Clause 5.2.4 gives these three their own Entity member, but each is
        // an Attribute the nested terms did not name.
        for member in ["id", "type", "location", "observationSpace", "operationSpace", "name"] {
            assert!(value.get(member).is_none(), "{member} is not a member the nested terms defined");
        }
    }

    /// A rendering with no projection at all narrows nothing at any depth.
    #[test]
    fn the_absent_projection_narrows_nothing() {
        let mut value = json!({"id": "urn:b", "type": "B", "name": {"type": "Property", "value": "y"}});

        NestedProjection::NONE.descend("rel").narrow(&mut value);

        assert!(value.get("name").is_some());
        assert!(NestedProjection::NONE.is_unrestricted());
    }
}
