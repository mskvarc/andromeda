//! The clause 4.21 selectors one request may not combine.
//!
//! `pick` reduces an Entity "down to only contain the specified Entity members" and `omit` removes them. No clause forbids naming both, and Tables 6.4.3.2-1 and 6.5.3.1-1 define them side by side, so a request carrying both keeps the picked members minus the omitted ones. A member named in both is the one combination that contradicts itself: its pick can select nothing, so the request is input data that does not meet the requirements of the operation and is refused. Clause 5.5.7 makes every name in the two lists a term of the request `@context`, so whether two names are one member is decided on their expansions: `name` and `https://schema.org/name` are one member under a context mapping the one to the other, and two short names are two members unless they expand alike. `attrs` (`attributes` in a Subscription) is by definition redundant with `pick`: Tables 6.4.3.2-1 and 5.2.14.1-1 call it "A synonym for pick, except that id, type, scope are not allowed. Deprecated"; combining it with either is refused on presence alone. The rules are the same wherever the three members appear: on the URL of an entity query, an entity retrieval or a temporal query, and inside the `notification` of a Subscription (Table 5.2.14.1-1) or of a Context Source Registration Subscription (clause 5.11.2.3, which takes the clause 5.2.12 data type unchanged).

use crate::{error::QueryError, projection::term::ProjectionTerm};
use thiserror::Error;

/// Whether a request supplied one of the projection selectors.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Presence {
    /// The member was absent from the request.
    #[default]
    Absent,
    /// The member was supplied.
    Present,
}

impl Presence {
    /// Whether an optional member was supplied.
    #[must_use]
    pub const fn of<T: ?Sized>(member: Option<&T>) -> Presence {
        match member {
            Some(_) => Presence::Present,
            None => Presence::Absent,
        }
    }

    /// Whether this is [`Presence::Present`].
    const fn is_present(self) -> bool {
        matches!(self, Presence::Present)
    }
}

/// Two clause 4.21 selectors a request carried that contradict each other.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum ProjectionConflict {
    /// `attrs` (`attributes` in a Subscription) is a synonym for `pick` (ETSI GS CIM 009 clause 4.21).
    #[error("pick/omit and attrs are mutually exclusive")]
    ProjectionWithAttrs,
}

/// Which of the clause 4.21 selectors one request carried.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ProjectionSelectors {
    /// The members the response is reduced to.
    pub pick: Presence,
    /// The members the response drops.
    pub omit: Presence,
    /// The deprecated synonym for `pick`.
    pub attrs: Presence,
}

/// Refuses a request that combined `pick` or `omit` with their deprecated synonym.
///
/// # Errors
///
/// Returns [`ProjectionConflict::ProjectionWithAttrs`] when `pick` or `omit` is combined with the deprecated synonym.
pub const fn check_projection_exclusivity(selectors: ProjectionSelectors) -> Result<(), ProjectionConflict> {
    if (selectors.pick.is_present() || selectors.omit.is_present()) && selectors.attrs.is_present() {
        return Err(ProjectionConflict::ProjectionWithAttrs);
    }
    Ok(())
}

/// Refuses an expanded projection whose `pick` and `omit` name one member.
///
/// # Errors
///
/// Returns [`QueryError::ProjectionMemberPickedAndOmitted`] naming the first member `omit` removes from what `pick` selects.
pub fn check_member_exclusivity(pick: &[ProjectionTerm], omit: &[ProjectionTerm]) -> Result<(), QueryError> {
    match first_member_in_both(pick, omit) {
        Some(member) => Err(QueryError::ProjectionMemberPickedAndOmitted {
            member: member.clone(), // clone: the refusal outlives the projection it was found in
        }),
        None => Ok(()),
    }
}

/// The first member `omit` removes from what `pick` selects at the same level.
///
/// A bare `omit` term removes the member itself. An `omit` term carrying a `{...}` group removes members inside the linked entity and leaves the Relationship (clause 4.21 `LinkedEntityTerm`), so it contradicts `pick` only where the pick of that Relationship carries a group naming the same members.
fn first_member_in_both<'t>(pick: &'t [ProjectionTerm], omit: &'t [ProjectionTerm]) -> Option<&'t ProjectionTerm> {
    omit.iter().find_map(|omitted| {
        pick.iter().filter(|picked| picked.names_same_member(omitted)).find_map(|picked| {
            if omitted.linked_terms().is_empty() {
                Some(omitted)
            } else {
                first_member_in_both(picked.linked_terms(), omitted.linked_terms())
            }
        })
    })
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    clippy::panic,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        error::QueryError,
        projection::{
            exclusivity::{Presence, ProjectionConflict, ProjectionSelectors, check_member_exclusivity, check_projection_exclusivity},
            term::ProjectionTerm,
        },
    };
    use iri_rs::IriBuf;
    use std::slice;

    fn selectors(pick: Presence, omit: Presence, attrs: Presence) -> Result<(), ProjectionConflict> {
        check_projection_exclusivity(ProjectionSelectors { pick, omit, attrs })
    }

    fn attribute(iri: &str, linked: Vec<ProjectionTerm>) -> ProjectionTerm {
        ProjectionTerm::Attribute {
            iri: IriBuf::new(iri.to_owned()).expect("valid IRI"),
            linked,
        }
    }

    fn leaf(iri: &str) -> ProjectionTerm {
        attribute(iri, Vec::new())
    }

    fn picked_and_omitted(pick: &[ProjectionTerm], omit: &[ProjectionTerm]) -> Option<ProjectionTerm> {
        match check_member_exclusivity(pick, omit) {
            Ok(()) => None,
            Err(QueryError::ProjectionMemberPickedAndOmitted { member }) => Some(member),
            Err(other) => panic!("unexpected refusal: {other:?}"),
        }
    }

    #[test]
    fn one_selector_alone_or_pick_with_omit_is_admitted() {
        for (pick, omit, attrs) in [
            (Presence::Present, Presence::Present, Presence::Absent),
            (Presence::Present, Presence::Absent, Presence::Absent),
            (Presence::Absent, Presence::Present, Presence::Absent),
            (Presence::Absent, Presence::Absent, Presence::Present),
            (Presence::Absent, Presence::Absent, Presence::Absent),
        ] {
            assert!(selectors(pick, omit, attrs).is_ok(), "{pick:?} {omit:?} {attrs:?}");
        }
    }

    #[test]
    fn the_deprecated_synonym_cannot_join_either_end() {
        for (pick, omit) in [
            (Presence::Present, Presence::Absent),
            (Presence::Absent, Presence::Present),
            (Presence::Present, Presence::Present),
        ] {
            pretty_assertions::assert_eq!(selectors(pick, omit, Presence::Present), Err(ProjectionConflict::ProjectionWithAttrs));
        }
    }

    #[test]
    fn pick_and_omit_of_other_members_are_admitted() {
        assert_eq!(
            picked_and_omitted(&[leaf("https://example.com/speed")], &[leaf("https://example.com/name")]),
            None
        );
    }

    #[test]
    fn an_attribute_named_in_both_is_refused() {
        let speed = leaf("https://example.com/speed");
        assert_eq!(
            picked_and_omitted(&[leaf("https://example.com/name"), speed.clone()], slice::from_ref(&speed)),
            Some(speed)
        );
    }

    /// Clause 5.5.7: `pick=name&omit=https://schema.org/name` under a context mapping `name` to `https://schema.org/name` expands both to one IRI, so the comparison sees only that IRI.
    #[test]
    fn two_spellings_expanding_to_one_attribute_are_refused() {
        let name = leaf("https://schema.org/name");
        assert_eq!(picked_and_omitted(slice::from_ref(&name), slice::from_ref(&name)), Some(name));
    }

    /// Two short names a context maps to different IRIs are different members.
    #[test]
    fn two_expansions_of_different_attributes_are_admitted() {
        assert_eq!(
            picked_and_omitted(&[leaf("https://schema.org/name")], &[leaf("https://example.com/name")]),
            None
        );
    }

    #[test]
    fn a_keyword_named_in_both_is_refused() {
        for keyword in [ProjectionTerm::Id, ProjectionTerm::Type, ProjectionTerm::Scope] {
            assert_eq!(picked_and_omitted(slice::from_ref(&keyword), slice::from_ref(&keyword)), Some(keyword));
        }
    }

    /// A bare `omit` term removes the Relationship a nested `pick` term keeps.
    #[test]
    fn a_bare_omit_of_a_relationship_picked_with_a_group_is_refused() {
        let owner = leaf("https://example.com/owner");
        let pick = [attribute("https://example.com/owner", vec![leaf("https://example.com/name")])];
        assert_eq!(picked_and_omitted(&pick, slice::from_ref(&owner)), Some(owner));
    }

    /// Clause 4.21: `omit=owner{name}` removes `name` inside the linked entity and keeps `owner`.
    #[test]
    fn an_omit_group_under_a_relationship_picked_whole_is_admitted() {
        let omit = [attribute("https://example.com/owner", vec![leaf("https://example.com/name")])];
        assert_eq!(picked_and_omitted(&[leaf("https://example.com/owner")], &omit), None);
    }

    #[test]
    fn groups_naming_different_linked_members_are_admitted() {
        let pick = [attribute("https://example.com/owner", vec![leaf("https://example.com/name")])];
        let omit = [attribute("https://example.com/owner", vec![leaf("https://example.com/email")])];
        assert_eq!(picked_and_omitted(&pick, &omit), None);
    }

    #[test]
    fn a_linked_member_named_in_both_groups_is_refused() {
        let email = leaf("https://example.com/email");
        let pick = [attribute("https://example.com/owner", vec![leaf("https://example.com/name"), email.clone()])];
        let omit = [attribute("https://example.com/owner", vec![email.clone()])];
        assert_eq!(picked_and_omitted(&pick, &omit), Some(email));
    }

    #[test]
    fn the_refusal_names_the_member_by_its_expanded_name() {
        let error = check_member_exclusivity(&[leaf("https://schema.org/name")], &[leaf("https://schema.org/name")]).expect_err("one member in both");
        assert_eq!(error.to_string(), "'https://schema.org/name' is named in both pick and omit");
    }

    #[test]
    fn presence_is_read_off_the_member() {
        pretty_assertions::assert_eq!(Presence::of(Some(&"pick")), Presence::Present);
        pretty_assertions::assert_eq!(Presence::of(Option::<&str>::None), Presence::Absent);
    }
}
