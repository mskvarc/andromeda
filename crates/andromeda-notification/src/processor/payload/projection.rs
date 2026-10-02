//! The attribute half of the clause 5.8.6 `notification.pick` /
//! `notification.omit` projection, decided on the notified entity state before
//! it is compacted for delivery.
//!
//! The entity state keys its attributes by IRI, which is how the compiled
//! [`Projection`] names them too, so the clause 4.21 attribute rule is a name
//! comparison here. `id`, `type` and `scope` are not attributes and are not
//! decided here: they exist as such only once compaction has written them, so
//! `StructuralProjection` prunes them afterwards, by the same rule a query
//! response uses. Clause 5.8.6 reduces "every Entity within the payload body
//! ... down to only contain the defined entity members listed", which is clause
//! 5.7.1.5's sentence verbatim, so the two must not read it differently.

use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_query::projection::{compiled::Projection, term::ProjectionTerm};

/// Whether a compiled `pick`/`omit` projection keeps the attribute `name`.
///
/// With a non-empty `pick` only the picked attributes survive; a non-empty
/// `omit` removes the attributes it names bare.
pub(crate) fn projection_keeps(name: &AttributeName, projection: &Projection) -> bool {
    (projection.pick.is_empty() || kept_by_pick(name, &projection.pick)) && (projection.omit.is_empty() || !removed_by_omit(name, &projection.omit))
}

/// Clause 4.21: a non-empty `pick` keeps the attributes it names.
fn kept_by_pick(name: &AttributeName, pick: &[ProjectionTerm]) -> bool {
    pick.iter()
        .any(|term| matches!(term, ProjectionTerm::Attribute { iri, .. } if iri.as_str() == name.as_str()))
}

/// Clause 4.21: `omit` removes the attributes it names bare. `omit=rel{a}`
/// omits `a` inside the entity `rel` links to, not `rel` itself, so a term
/// carrying a linked sub-projection removes nothing here.
fn removed_by_omit(name: &AttributeName, omit: &[ProjectionTerm]) -> bool {
    omit.iter()
        .any(|term| matches!(term, ProjectionTerm::Attribute { iri, linked } if iri.as_str() == name.as_str() && linked.is_empty()))
}

#[cfg(test)]
mod tests {
    use crate::processor::payload::projection::projection_keeps;
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use andromeda_ngsi_ld_query::projection::{compiled::Projection, term::ProjectionTerm};
    use iri_rs::IriBuf;

    const SPEED: &str = "https://example.com/speed";
    const COLOUR: &str = "https://example.com/colour";

    fn attribute(iri: &str, linked: Vec<ProjectionTerm>) -> ProjectionTerm {
        ProjectionTerm::Attribute {
            iri: IriBuf::new(iri.to_owned()).expect("valid IRI"),
            linked,
        }
    }

    fn name(iri: &str) -> AttributeName {
        iri.parse().expect("valid attribute name")
    }

    #[test]
    fn an_empty_projection_keeps_every_attribute() {
        assert!(projection_keeps(&name(SPEED), &Projection::default()));
        assert!(projection_keeps(&name(COLOUR), &Projection::default()));
    }

    /// Clause 5.8.6 / 4.21: a pick reduces the entity to the picked attribute.
    #[test]
    fn a_pick_keeps_only_the_picked_attribute() {
        let projection = Projection::builder().pick(vec![attribute(SPEED, Vec::new())]).build();
        assert!(projection_keeps(&name(SPEED), &projection));
        assert!(!projection_keeps(&name(COLOUR), &projection));
    }

    /// A pick that names only a structural keyword removes every attribute and
    /// leaves the three structural members for the pass that owns them.
    #[test]
    fn a_pick_naming_only_a_keyword_removes_every_attribute() {
        let projection = Projection::builder().pick(vec![ProjectionTerm::Scope]).build();
        assert!(!projection_keeps(&name(SPEED), &projection));
        assert!(!projection_keeps(&name(COLOUR), &projection));
    }

    #[test]
    fn an_omit_removes_the_named_attribute_and_nothing_else() {
        let projection = Projection::builder().omit(vec![attribute(COLOUR, Vec::new())]).build();
        assert!(!projection_keeps(&name(COLOUR), &projection));
        assert!(projection_keeps(&name(SPEED), &projection));

        let projection = Projection::builder().omit(vec![ProjectionTerm::Scope]).build();
        assert!(projection_keeps(&name(SPEED), &projection) && projection_keeps(&name(COLOUR), &projection));
    }

    /// Clause 4.21: `omit=rel{a}` reaches into the linked entity and leaves
    /// the relationship itself in place.
    #[test]
    fn an_omit_term_with_a_linked_sub_projection_keeps_the_attribute_itself() {
        let projection = Projection::builder().omit(vec![attribute(SPEED, vec![attribute(COLOUR, Vec::new())])]).build();
        assert!(projection_keeps(&name(SPEED), &projection));
    }
}
