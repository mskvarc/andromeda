//! The clause 4.21 selectors one request may not combine.
//!
//! Three members shape the same projection and a request that supplies two of
//! them says two contradictory things. `pick` reduces an Entity "down to only
//! contain the specified Entity members" and `omit` removes them, so the two
//! select opposite ends of one projection; `attrs` -- `attributes` in a
//! Subscription -- is by definition redundant with `pick`, which Tables
//! 6.4.3.2-1 and 5.2.14.1-1 say in the same words: "A synonym for pick, except
//! that id, type, scope are not allowed. Deprecated".
//!
//! The rule is the same wherever the three members appear: on the URL of an
//! entity query, an entity retrieval or a temporal query, and inside the
//! `notification` of a Subscription (Table 5.2.14.1-1) or of a Context Source
//! Registration Subscription (clause 5.11.2.3, which takes the clause 5.2.12
//! data type unchanged).

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
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum ProjectionConflict {
    /// `pick` and `omit` select opposite ends of the same projection (ETSI GS CIM 009 clause 4.21).
    #[error("pick and omit are mutually exclusive")]
    PickWithOmit,
    /// `attrs` (`attributes` in a Subscription) is a synonym for `pick` (ETSI GS CIM 009 clause 4.21).
    #[error("pick/omit and attrs are mutually exclusive")]
    ProjectionWithAttrs,
}

/// The clause 4.21 selectors one request carried.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ProjectionSelectors {
    /// The members the response is reduced to.
    pub pick: Presence,
    /// The members the response drops.
    pub omit: Presence,
    /// The deprecated synonym for `pick`.
    pub attrs: Presence,
}

/// Refuse a request that combined selectors shaping the same projection.
///
/// # Errors
///
/// Returns [`ProjectionConflict::PickWithOmit`] when both ends of the
/// projection are named, and [`ProjectionConflict::ProjectionWithAttrs`] when
/// either is combined with the deprecated synonym.
pub const fn check_projection_exclusivity(selectors: ProjectionSelectors) -> Result<(), ProjectionConflict> {
    if selectors.pick.is_present() && selectors.omit.is_present() {
        return Err(ProjectionConflict::PickWithOmit);
    }
    if (selectors.pick.is_present() || selectors.omit.is_present()) && selectors.attrs.is_present() {
        return Err(ProjectionConflict::ProjectionWithAttrs);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::projection::exclusivity::{Presence, ProjectionConflict, ProjectionSelectors, check_projection_exclusivity};

    fn selectors(pick: Presence, omit: Presence, attrs: Presence) -> ProjectionSelectors {
        ProjectionSelectors { pick, omit, attrs }
    }

    /// Any one selector on its own shapes one projection and is admitted.
    #[test]
    fn one_selector_alone_is_admitted() {
        for supplied in [
            selectors(Presence::Present, Presence::Absent, Presence::Absent),
            selectors(Presence::Absent, Presence::Present, Presence::Absent),
            selectors(Presence::Absent, Presence::Absent, Presence::Present),
            selectors(Presence::Absent, Presence::Absent, Presence::Absent),
        ] {
            assert!(check_projection_exclusivity(supplied).is_ok(), "{supplied:?}");
        }
    }

    /// `pick` and `omit` select opposite ends of one projection.
    #[test]
    fn pick_and_omit_together_are_refused() {
        pretty_assertions::assert_eq!(
            check_projection_exclusivity(selectors(Presence::Present, Presence::Present, Presence::Absent)),
            Err(ProjectionConflict::PickWithOmit)
        );
    }

    /// The deprecated synonym cannot stand beside either end.
    #[test]
    fn the_deprecated_synonym_cannot_join_either_end() {
        pretty_assertions::assert_eq!(
            check_projection_exclusivity(selectors(Presence::Present, Presence::Absent, Presence::Present)),
            Err(ProjectionConflict::ProjectionWithAttrs)
        );
        pretty_assertions::assert_eq!(
            check_projection_exclusivity(selectors(Presence::Absent, Presence::Present, Presence::Present)),
            Err(ProjectionConflict::ProjectionWithAttrs)
        );
    }

    /// All three at once is still first of all a `pick`/`omit` contradiction.
    #[test]
    fn all_three_report_the_opposite_ends_first() {
        pretty_assertions::assert_eq!(
            check_projection_exclusivity(selectors(Presence::Present, Presence::Present, Presence::Present)),
            Err(ProjectionConflict::PickWithOmit)
        );
    }

    /// The presence of a member is read off the option that carries it.
    #[test]
    fn presence_is_read_off_the_member() {
        pretty_assertions::assert_eq!(Presence::of(Some(&"pick")), Presence::Present);
        pretty_assertions::assert_eq!(Presence::of(Option::<&str>::None), Presence::Absent);
    }
}
