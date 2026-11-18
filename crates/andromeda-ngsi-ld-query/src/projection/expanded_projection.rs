//! The compiled `pick` / `omit` projection of a query (clause 4.21).

use crate::projection::expanded_projection_term::ExpandedProjectionTerm;
use typed_builder::TypedBuilder;

/// Which members of a matching entity reach the response.
///
/// Clause 4.21: a member reaches the response when `pick` is empty or names it and `omit` does not name it. Both shape the representation of the selected entities; the one way they reach the selection is through an Entity they would reduce to no member at all, which [`ProjectedMembers`] states as a selection criterion of its own.
///
/// [`ProjectedMembers`]: crate::projection::projected_members::ProjectedMembers
#[derive(Debug, Clone, Default, PartialEq, Eq, TypedBuilder)]
#[builder(field_defaults(default = Default::default()))]
pub struct ExpandedProjection {
    /// Terms to include. Empty means every member.
    pub pick: Vec<ExpandedProjectionTerm>,
    /// Terms to exclude. Empty means none.
    pub omit: Vec<ExpandedProjectionTerm>,
}

impl ExpandedProjection {
    /// Whether a structural member survives the clause 4.21 keyword rule: a
    /// non-empty `pick` keeps it only when it is listed, and an `omit` drops it
    /// when it is.
    #[must_use]
    pub fn keeps(&self, keyword: &ExpandedProjectionTerm) -> bool {
        if !self.pick.is_empty() && !self.pick.contains(keyword) {
            return false;
        }
        !self.omit.contains(keyword)
    }

    /// Whether the request named a projection at all.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.pick.is_empty() && self.omit.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use crate::projection::{expanded_projection::ExpandedProjection, expanded_projection_term::ExpandedProjectionTerm};

    /// Clause 4.21: a non-empty `pick` keeps a structural member only when it
    /// lists it.
    #[test]
    fn a_pick_keeps_only_the_keywords_it_lists() {
        let projection = ExpandedProjection::builder().pick(vec![ExpandedProjectionTerm::Id]).build();
        assert!(projection.keeps(&ExpandedProjectionTerm::Id));
        assert!(!projection.keeps(&ExpandedProjectionTerm::Type));
        assert!(!projection.keeps(&ExpandedProjectionTerm::Scope));
    }

    /// Clause 4.21: an `omit` drops the structural members it lists and keeps
    /// the rest.
    #[test]
    fn an_omit_drops_only_the_keywords_it_lists() {
        let projection = ExpandedProjection::builder().omit(vec![ExpandedProjectionTerm::Type]).build();
        assert!(projection.keeps(&ExpandedProjectionTerm::Id));
        assert!(!projection.keeps(&ExpandedProjectionTerm::Type));
        assert!(projection.keeps(&ExpandedProjectionTerm::Scope));
    }

    /// A request that named no projection keeps every member.
    #[test]
    fn no_projection_keeps_every_keyword() {
        let projection = ExpandedProjection::default();
        assert!(projection.is_empty());
        for keyword in [ExpandedProjectionTerm::Id, ExpandedProjectionTerm::Type, ExpandedProjectionTerm::Scope] {
            assert!(projection.keeps(&keyword), "{keyword:?}");
        }
    }
}
