//! The clause 4.21 structural-member rule.
//!
//! `pick` and `omit` may name `id`, `type` and `scope` as keywords alongside
//! attributes (clause 4.21). Read literally, a non-empty `pick` keeps one
//! of these three members only when it is explicitly listed, and a non-empty
//! `omit` drops whichever of the three it lists. `@context` is a JSON-LD
//! transport concern, never a structural member, and this rule never touches
//! it.
//!
//! The rule is not specific to the non-temporal entity representation: a
//! temporal representation carries the same `id` / `type` members and is
//! written under the same rule. The type is public so every writer reuses it
//! rather than reimplementing the rule.

use andromeda_ngsi_ld_query::projection::{compiled::Projection, term::ProjectionTerm};

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

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::structural_projection::StructuralProjection;
    use andromeda_ngsi_ld_query::projection::{compiled::Projection, term::ProjectionTerm};
    use iri_rs::IriBuf;

    fn iri(value: &str) -> IriBuf {
        IriBuf::new(value.to_string()).expect("valid IRI")
    }

    /// Clause 4.21, literal reading: a non-empty `pick` naming only `id`
    /// keeps `id` and drops every other structural member.
    #[test]
    fn pick_of_id_keeps_only_id() {
        let projection = Projection {
            pick: vec![ProjectionTerm::Id],
            omit: Vec::new(),
        };
        let rule = StructuralProjection::from(&projection);
        assert!(rule.keeps(&ProjectionTerm::Id));
        assert!(!rule.keeps(&ProjectionTerm::Type));
        assert!(!rule.keeps(&ProjectionTerm::Scope));
    }

    /// Clause 4.21: `omit` removes the structural members it lists.
    #[test]
    fn omit_of_scope_removes_scope() {
        let projection = Projection {
            pick: Vec::new(),
            omit: vec![ProjectionTerm::Scope],
        };
        let rule = StructuralProjection::from(&projection);
        assert!(rule.keeps(&ProjectionTerm::Id));
        assert!(rule.keeps(&ProjectionTerm::Type));
        assert!(!rule.keeps(&ProjectionTerm::Scope));
    }

    /// Clause 4.21: `omit` removes `type` when it is listed.
    #[test]
    fn omit_of_type_removes_type() {
        let projection = Projection {
            pick: Vec::new(),
            omit: vec![ProjectionTerm::Type],
        };
        let rule = StructuralProjection::from(&projection);
        assert!(rule.keeps(&ProjectionTerm::Id));
        assert!(!rule.keeps(&ProjectionTerm::Type));
    }

    /// Clause 4.21, literal reading: a non-empty `pick` that names only an
    /// attribute leaves `id` and `type` out, since neither keyword was listed.
    #[test]
    fn pick_of_an_attribute_leaves_id_and_type_out() {
        let projection = Projection {
            pick: vec![ProjectionTerm::Attribute {
                iri: iri("https://example.com/speed"),
                linked: Vec::new(),
            }],
            omit: Vec::new(),
        };
        let rule = StructuralProjection::from(&projection);
        assert!(!rule.keeps(&ProjectionTerm::Id));
        assert!(!rule.keeps(&ProjectionTerm::Type));
    }

    /// An empty `pick` and empty `omit` select every member with no
    /// restriction, so the structural members are kept.
    #[test]
    fn an_empty_projection_keeps_every_structural_member() {
        let projection = Projection::default();
        let rule = StructuralProjection::from(&projection);
        for keyword in [ProjectionTerm::Id, ProjectionTerm::Type, ProjectionTerm::Scope] {
            assert!(rule.keeps(&keyword), "{keyword:?}");
        }
    }
}
