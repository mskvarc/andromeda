//! The clause 4.21 selectors one request may not combine.
//!
//! `pick` reduces an Entity "down to only contain the specified Entity members" and `omit` removes them. No clause forbids naming both, and Tables 6.4.3.2-1 and 6.5.3.1-1 define them side by side, so a request carrying both keeps the picked members minus the omitted ones. A member named in both is the one combination that contradicts itself: its pick can select nothing, so the request is input data that does not meet the requirements of the operation and is refused. `attrs` (`attributes` in a Subscription) is by definition redundant with `pick`: Tables 6.4.3.2-1 and 5.2.14.1-1 call it "A synonym for pick, except that id, type, scope are not allowed. Deprecated". The rule is the same wherever the three members appear: on the URL of an entity query, an entity retrieval or a temporal query, and inside the `notification` of a Subscription (Table 5.2.14.1-1) or of a Context Source Registration Subscription (clause 5.11.2.3, which takes the clause 5.2.12 data type unchanged).

use crate::projection::grammar::RawProjectionTerm;
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
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
    /// `pick` and `omit` name the same member of the same Entity, so its pick selects nothing.
    #[error("'{member}' is named in both pick and omit")]
    MemberPickedAndOmitted {
        /// The member as the request wrote it, at the nesting level both lists name it.
        member: AttributeName,
    },
}

/// The clause 4.21 selectors one request carried.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ProjectionSelectors<'t> {
    /// The members the response is reduced to.
    pub pick: Option<&'t [RawProjectionTerm]>,
    /// The members the response drops.
    pub omit: Option<&'t [RawProjectionTerm]>,
    /// The deprecated synonym for `pick`.
    pub attrs: Presence,
}

/// Refuses a request that combined selectors shaping the same projection.
///
/// # Errors
///
/// Returns [`ProjectionConflict::ProjectionWithAttrs`] when `pick` or `omit` is combined with the deprecated synonym, and [`ProjectionConflict::MemberPickedAndOmitted`] when both name one member.
pub fn check_projection_exclusivity(selectors: ProjectionSelectors<'_>) -> Result<(), ProjectionConflict> {
    if (selectors.pick.is_some() || selectors.omit.is_some()) && selectors.attrs.is_present() {
        return Err(ProjectionConflict::ProjectionWithAttrs);
    }
    match first_member_in_both(selectors.pick.unwrap_or_default(), selectors.omit.unwrap_or_default()) {
        Some(member) => Err(ProjectionConflict::MemberPickedAndOmitted { member: member.clone() }),
        None => Ok(()),
    }
}

/// The first member `omit` removes from what `pick` selects at the same level.
///
/// A bare `omit` term removes the member itself. An `omit` term carrying a `{...}` group removes members inside the linked entity and leaves the Relationship (clause 4.21 `LinkedEntityTerm`), so it contradicts `pick` only where the pick of that Relationship carries a group naming the same members.
fn first_member_in_both<'t>(pick: &'t [RawProjectionTerm], omit: &'t [RawProjectionTerm]) -> Option<&'t AttributeName> {
    omit.iter().find_map(|omitted| {
        pick.iter().filter(|picked| picked.name == omitted.name).find_map(|picked| {
            if omitted.linked.is_empty() {
                Some(&omitted.name)
            } else {
                first_member_in_both(&picked.linked, &omitted.linked)
            }
        })
    })
}

#[cfg(test)]
mod tests {
    use crate::projection::{
        exclusivity::{Presence, ProjectionConflict, ProjectionSelectors, check_projection_exclusivity},
        grammar::{ProjectionList, RawProjectionTerm},
    };

    fn terms(raw: &str) -> Vec<RawProjectionTerm> {
        raw.parse::<ProjectionList>().map(Vec::from).expect("valid projection list")
    }

    fn checked(pick: Option<&str>, omit: Option<&str>, attrs: Presence) -> Result<(), ProjectionConflict> {
        let pick = pick.map(terms);
        let omit = omit.map(terms);
        check_projection_exclusivity(ProjectionSelectors {
            pick: pick.as_deref(),
            omit: omit.as_deref(),
            attrs,
        })
    }

    fn picked_and_omitted(member: &str) -> Result<(), ProjectionConflict> {
        Err(ProjectionConflict::MemberPickedAndOmitted {
            member: member.parse().expect("valid attribute name"),
        })
    }

    #[test]
    fn one_selector_alone_or_pick_with_omit_of_other_members_is_admitted() {
        for (pick, omit, attrs) in [
            (Some("speed"), Some("name"), Presence::Absent),
            (Some("speed"), None, Presence::Absent),
            (None, Some("speed"), Presence::Absent),
            (None, None, Presence::Present),
            (None, None, Presence::Absent),
        ] {
            assert!(checked(pick, omit, attrs).is_ok(), "{pick:?} {omit:?} {attrs:?}");
        }
    }

    #[test]
    fn the_deprecated_synonym_cannot_join_either_end() {
        pretty_assertions::assert_eq!(checked(Some("speed"), None, Presence::Present), Err(ProjectionConflict::ProjectionWithAttrs));
        pretty_assertions::assert_eq!(checked(None, Some("speed"), Presence::Present), Err(ProjectionConflict::ProjectionWithAttrs));
    }

    #[test]
    fn all_three_report_the_deprecated_synonym() {
        pretty_assertions::assert_eq!(
            checked(Some("speed"), Some("speed"), Presence::Present),
            Err(ProjectionConflict::ProjectionWithAttrs)
        );
    }

    #[test]
    fn an_attribute_named_in_both_is_refused() {
        pretty_assertions::assert_eq!(checked(Some("name,speed"), Some("speed"), Presence::Absent), picked_and_omitted("speed"));
    }

    #[test]
    fn a_keyword_named_in_both_is_refused() {
        for keyword in ["id", "type", "scope"] {
            pretty_assertions::assert_eq!(checked(Some(keyword), Some(keyword), Presence::Absent), picked_and_omitted(keyword));
        }
    }

    /// A bare `omit` term removes the Relationship a nested `pick` term keeps.
    #[test]
    fn a_bare_omit_of_a_relationship_picked_with_a_group_is_refused() {
        pretty_assertions::assert_eq!(checked(Some("owner{name}"), Some("owner"), Presence::Absent), picked_and_omitted("owner"));
    }

    /// Clause 4.21: `omit=owner{name}` removes `name` inside the linked entity and keeps `owner`.
    #[test]
    fn an_omit_group_under_a_relationship_picked_whole_is_admitted() {
        assert!(checked(Some("owner"), Some("owner{name}"), Presence::Absent).is_ok());
    }

    #[test]
    fn groups_naming_different_linked_members_are_admitted() {
        assert!(checked(Some("owner{name}"), Some("owner{email}"), Presence::Absent).is_ok());
    }

    #[test]
    fn a_linked_member_named_in_both_groups_is_refused() {
        pretty_assertions::assert_eq!(
            checked(Some("owner{name,email}"), Some("owner{email}"), Presence::Absent),
            picked_and_omitted("email")
        );
    }

    #[test]
    fn presence_is_read_off_the_member() {
        pretty_assertions::assert_eq!(Presence::of(Some(&"pick")), Presence::Present);
        pretty_assertions::assert_eq!(Presence::of(Option::<&str>::None), Presence::Absent);
    }
}
