//! The number the `posted_queries.kind` column stores a posted query's API as.
//!
//! Written as an exhaustive match, so an API added to the domain fails to
//! compile here, where its persisted number is decided. A stored kind is never
//! read back as an API: a query is read through the API that asks for it.

use andromeda_posted_query::posted_query::PostedQueryKind;
use andromeda_postgresql_codes::posted_query_kind::PostedQueryKindCode;

/// The registry member the store persists for a posted query's API.
#[must_use]
pub const fn stored_kind(kind: PostedQueryKind) -> PostedQueryKindCode {
    match kind {
        PostedQueryKind::Entities => PostedQueryKindCode::Entities,
        PostedQueryKind::Temporal => PostedQueryKindCode::Temporal,
    }
}

#[cfg(test)]
mod tests {
    use crate::posted_query::kind_code::stored_kind;
    use andromeda_posted_query::posted_query::PostedQueryKind;
    use andromeda_postgresql_codes::posted_query_kind::PostedQueryKindCode;
    use pretty_assertions::assert_eq;
    use strum::IntoEnumIterator;

    /// The registry's label is the token the domain enum renders, so every API
    /// is stored under the number its own token is registered under.
    #[test]
    fn every_api_is_stored_under_the_number_its_token_is_registered_under() {
        for kind in PostedQueryKind::iter() {
            assert_eq!(stored_kind(kind).label(), kind.to_string());
        }
        assert_eq!(PostedQueryKindCode::ALL.len(), PostedQueryKind::iter().count());
    }
}
