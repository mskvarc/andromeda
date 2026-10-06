//! Whether the clause 4.21 rule in force keeps one structural member of an
//! entity.

use crate::structural_projection::StructuralProjection;
use andromeda_linked_retrieval::{keyword::Keyword, narrowing::member::MemberRule};

/// Whether the rule in force keeps `keyword`: a Linked Entity's own rule (the
/// list its path decided), or else the request's.
pub(crate) fn keeps(narrowing: Option<MemberRule<'_>>, projection: Option<StructuralProjection<'_>>, keyword: Keyword) -> bool {
    match (narrowing, projection) {
        (Some(rule), _) => rule.keyword(keyword).is_write(),
        (None, Some(projection)) => projection.keeps(&keyword.term()),
        (None, None) => true,
    }
}
