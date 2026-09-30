//! The relation that stores JSON-LD `@context` documents.
//!
//! `jsonld_contexts` holds every hosted, implicitly-created and remotely-cached
//! Context the resolver serves or fetched, keyed by both its local id and its
//! source URL so a lookup can match either identity.

use sea_query::Iden;

/// The `jsonld_contexts` table - stored JSON-LD `@context` documents.
#[derive(Iden, Clone, Copy)]
pub enum JsonldContexts {
    /// Table name: `jsonld_contexts`
    Table,
    /// Local Context identifier (an NGSI-LD URN)
    Id,
    /// Source URL, when the Context has one
    Url,
    /// The URL a Cached Context was finally read from
    DocumentUrl,
    /// Context kind (`JsonldContextKindCode`)
    Kind,
    /// The `@context` document
    Payload,
    /// Instant the row was created
    CreatedAt,
    /// Instant the Context was last served
    LastUsage,
    /// Number of times the Context has been served
    NumberOfHits,
    /// Instant the Context expires, when it has one
    ExpiresAt,
    /// Optional caller-supplied metadata
    ExtraInfo,
}

#[cfg(test)]
mod tests {
    use crate::jsonld::JsonldContexts;
    use pretty_assertions::assert_eq;
    use sea_query::Iden;

    #[test]
    fn jsonld_contexts_table_and_columns() {
        assert_eq!(JsonldContexts::Table.to_string(), "jsonld_contexts");
        assert_eq!(JsonldContexts::Id.to_string(), "id");
        assert_eq!(JsonldContexts::Url.to_string(), "url");
        assert_eq!(JsonldContexts::DocumentUrl.to_string(), "document_url");
        assert_eq!(JsonldContexts::Kind.to_string(), "kind");
        assert_eq!(JsonldContexts::Payload.to_string(), "payload");
        assert_eq!(JsonldContexts::CreatedAt.to_string(), "created_at");
        assert_eq!(JsonldContexts::LastUsage.to_string(), "last_usage");
        assert_eq!(JsonldContexts::NumberOfHits.to_string(), "number_of_hits");
        assert_eq!(JsonldContexts::ExpiresAt.to_string(), "expires_at");
        assert_eq!(JsonldContexts::ExtraInfo.to_string(), "extra_info");
    }
}
