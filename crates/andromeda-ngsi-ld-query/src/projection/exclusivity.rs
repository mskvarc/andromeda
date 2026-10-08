//! The clause 4.21 selectors one request may not combine.
//!
//! `pick` reduces an Entity "down to only contain the specified Entity members" and `omit` removes them. No clause forbids naming both, and Tables 6.4.3.2-1 and 6.5.3.1-1 define them side by side, so a request carrying both keeps the picked members minus the omitted ones. `attrs` (`attributes` in a Subscription) is by definition redundant with `pick`: Tables 6.4.3.2-1 and 5.2.14.1-1 call it "A synonym for pick, except that id, type, scope are not allowed. Deprecated". The rule is the same wherever the three members appear: on the URL of an entity query, an entity retrieval or a temporal query, and inside the `notification` of a Subscription (Table 5.2.14.1-1) or of a Context Source Registration Subscription (clause 5.11.2.3, which takes the clause 5.2.12 data type unchanged).

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

/// Refuses a request that combined selectors shaping the same projection.
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

#[cfg(test)]
mod tests {
    use crate::projection::exclusivity::{Presence, ProjectionConflict, ProjectionSelectors, check_projection_exclusivity};

    fn selectors(pick: Presence, omit: Presence, attrs: Presence) -> ProjectionSelectors {
        ProjectionSelectors { pick, omit, attrs }
    }

    /// Any one selector on its own, and `pick` with `omit`, is admitted.
    #[test]
    fn one_selector_alone_or_pick_with_omit_is_admitted() {
        for supplied in [
            selectors(Presence::Present, Presence::Present, Presence::Absent),
            selectors(Presence::Present, Presence::Absent, Presence::Absent),
            selectors(Presence::Absent, Presence::Present, Presence::Absent),
            selectors(Presence::Absent, Presence::Absent, Presence::Present),
            selectors(Presence::Absent, Presence::Absent, Presence::Absent),
        ] {
            assert!(check_projection_exclusivity(supplied).is_ok(), "{supplied:?}");
        }
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

    /// All three at once still carries the deprecated synonym beside a projection.
    #[test]
    fn all_three_report_the_deprecated_synonym() {
        pretty_assertions::assert_eq!(
            check_projection_exclusivity(selectors(Presence::Present, Presence::Present, Presence::Present)),
            Err(ProjectionConflict::ProjectionWithAttrs)
        );
    }

    /// The presence of a member is read off the option that carries it.
    #[test]
    fn presence_is_read_off_the_member() {
        pretty_assertions::assert_eq!(Presence::of(Some(&"pick")), Presence::Present);
        pretty_assertions::assert_eq!(Presence::of(Option::<&str>::None), Presence::Absent);
    }
}
