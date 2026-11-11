//! The relation a posted query occupies.
//!
//! A `posted_queries` row holds one paged query a client sent by POST, so the
//! page links of its result can name it by its digest (clauses 5.5.9 and
//! 6.3.10). The digest is the key: posting the same query again finds the row
//! it already has.

use sea_query::Iden;

/// The `posted_queries` table - one held query per row.
#[derive(Iden, Clone, Copy)]
pub enum PostedQueries {
    /// Table name: `posted_queries`
    Table,
    /// The API the query was posted to, as its compact code.
    Kind,
    /// Expiry instant, after which the query is gone.
    ExpiresAt,
    /// The digest of the query, its API and its `@context`, spelled as a page
    /// link carries it.
    Digest,
    /// The clause 5.2.23 Query, without its `@context`.
    Query,
    /// The context the POST named in its `Link` header, if it named one there.
    ContextLink,
    /// The `@context` member of the POST body, if it named one there.
    ContextBody,
}

#[cfg(test)]
mod tests {
    use crate::posted_query::PostedQueries;
    use pretty_assertions::assert_eq;
    use sea_query::Iden;

    #[test]
    fn posted_query_relation_and_column_names() {
        assert_eq!(PostedQueries::Table.to_string(), "posted_queries");
        assert_eq!(PostedQueries::Kind.to_string(), "kind");
        assert_eq!(PostedQueries::ExpiresAt.to_string(), "expires_at");
        assert_eq!(PostedQueries::Digest.to_string(), "digest");
        assert_eq!(PostedQueries::Query.to_string(), "query");
        assert_eq!(PostedQueries::ContextLink.to_string(), "context_link");
        assert_eq!(PostedQueries::ContextBody.to_string(), "context_body");
    }
}
