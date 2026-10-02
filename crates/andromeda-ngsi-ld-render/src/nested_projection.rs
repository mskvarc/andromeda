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
//!
//! Only Attributes and the three structural members are narrowed. `@context`
//! is a JSON-LD transport concern, and whether the clause 4.8 system
//! timestamps and the clause 4.22 expiry appear is the `sysAttrs`
//! representation option's decision, orthogonal to the projection; the walk
//! writes them whatever the nested terms say, as `join=flat`, which narrows the
//! typed entity, leaves them too.

use crate::structural_projection::StructuralProjection;
use andromeda_ngsi_ld_query::projection::{attribute_iri_index::AttributeIriIndex, compiled::Projection, matching::matching_term, term::ProjectionTerm};

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
    pub(crate) const fn structural(&self) -> StructuralProjection<'a> {
        StructuralProjection::new(self.pick, self.omit)
    }

    /// Whether this projection keeps the attribute keyed `key` of a linked
    /// entity.
    pub(crate) fn keeps_attribute(&self, key: &str) -> bool {
        let Some(index) = self.index else {
            return true;
        };
        let picked = self.pick.is_empty() || matching_term(key, self.pick, index).is_some();
        // Clause 4.21: `omit=rel{a}` omits `a` inside whatever `rel` links to,
        // so `rel` itself survives and the omission travels one level further
        // down.
        let omitted = matching_term(key, self.omit, index).is_some_and(|term| term.linked_terms().is_empty());
        picked && !omitted
    }
}

#[cfg(test)]
mod tests {
    use crate::nested_projection::{LinkedProjection, NestedProjection};
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use andromeda_ngsi_ld_query::projection::{attribute_iri_index::AttributeIriIndex, compiled::Projection, term::ProjectionTerm};
    use iri_rs::IriBuf;

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
    /// carried, which keeps of the entity `rel` links to only the members its
    /// terms defined (clause 5.7.1.5) -- not even `id` and `type`.
    #[test]
    fn descending_through_a_relationship_narrows_its_target_to_the_nested_terms() {
        let projection = Projection {
            pick: vec![nested("rel", vec![leaf("category")])],
            omit: Vec::new(),
        };
        let index = index(&["rel", "category", "name"]);
        let target = NestedProjection::root(LinkedProjection {
            projection: &projection,
            index: &index,
        })
        .descend("rel");

        assert!(target.keeps_attribute("category"));
        assert!(!target.keeps_attribute("name"));
        assert!(!target.keeps_attribute("location"));
        assert!(!target.structural().keeps(&ProjectionTerm::Id));
        assert!(!target.structural().keeps(&ProjectionTerm::Type));
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
        let target = NestedProjection::root(LinkedProjection {
            projection: &projection,
            index: &index,
        })
        .descend("relB");
        assert!(target.is_unrestricted());
        assert!(target.keeps_attribute("name"));
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
        let target = NestedProjection::root(LinkedProjection {
            projection: &projection,
            index: &index,
        })
        .descend("rel");
        assert!(target.keeps_attribute("category"));
        assert!(!target.keeps_attribute("name"));
        assert!(target.structural().keeps(&ProjectionTerm::Id));
    }

    /// Clause 4.21: `omit=rel{a}` omits `a` inside what `rel` links to, so a
    /// nested `omit` term that itself carries a sub-projection keeps the
    /// Relationship and travels one level further down.
    #[test]
    fn an_omit_term_carrying_a_sub_projection_keeps_the_relationship() {
        let projection = Projection {
            pick: Vec::new(),
            omit: vec![nested("outer", vec![nested("inner", vec![leaf("name")])])],
        };
        let index = index(&["outer", "inner", "name"]);
        let level_one = NestedProjection::root(LinkedProjection {
            projection: &projection,
            index: &index,
        })
        .descend("outer");
        assert!(level_one.keeps_attribute("inner"));
        assert!(!level_one.descend("inner").keeps_attribute("name"));
    }

    /// Clause 4.21 reduces a linked entity to the named attributes; `scope`
    /// is not on that list unless the nested terms name its keyword. The
    /// counterpart flat-side test is `andromeda-entity`'s
    /// `the_flat_narrowing_leaves_the_same_linked_entity_shape_as_the_inline_one`;
    /// the two must agree member for member.
    #[test]
    fn a_nested_pick_drops_scope_unless_the_scope_keyword_is_nested_too() {
        let index = index(&["rel", "category"]);
        let without_keyword = Projection {
            pick: vec![nested("rel", vec![leaf("category")])],
            omit: Vec::new(),
        };
        let with_keyword = Projection {
            pick: vec![nested("rel", vec![leaf("category"), ProjectionTerm::Scope])],
            omit: Vec::new(),
        };
        let scope_kept = |projection: &Projection| {
            NestedProjection::root(LinkedProjection { projection, index: &index })
                .descend("rel")
                .structural()
                .keeps(&ProjectionTerm::Scope)
        };
        assert!(!scope_kept(&without_keyword));
        assert!(scope_kept(&with_keyword));
    }

    /// A rendering with no projection at all narrows nothing at any depth.
    #[test]
    fn the_absent_projection_narrows_nothing() {
        let target = NestedProjection::NONE.descend("rel");
        assert!(target.is_unrestricted());
        assert!(target.keeps_attribute("name"));
    }
}
