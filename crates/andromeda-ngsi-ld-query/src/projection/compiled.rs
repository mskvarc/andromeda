//! The compiled `pick` / `omit` projection of a query (clause 4.21).

use crate::projection::term::ProjectionTerm;
use typed_builder::TypedBuilder;

/// Which members of a matching entity reach the response.
///
/// Clause 4.21 defines `pick` and `omit` as mutually exclusive, and both as
/// purely presentational: neither changes which entities match, which is why
/// they are held apart from the selection.
#[derive(Debug, Clone, Default, PartialEq, Eq, TypedBuilder)]
#[builder(field_defaults(default = Default::default()))]
pub struct Projection {
    /// Terms to include. Empty means every member.
    pub pick: Vec<ProjectionTerm>,
    /// Terms to exclude. Empty means none.
    pub omit: Vec<ProjectionTerm>,
}
