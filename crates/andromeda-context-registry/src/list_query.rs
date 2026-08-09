//! Parameters for listing hosted and cached Contexts.

use crate::detail_level::ContextDetailLevel;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::jsonld_context::ContextKind;

/// Keyset-paginated Context list parameters owned by registry policy.
#[derive(Debug)]
pub struct ContextListQuery<'a> {
    /// Optional Context-kind filter.
    pub kind: Option<&'a ContextKind>,
    /// Requested response projection.
    pub detail_level: ContextDetailLevel,
    /// Return rows whose local identifier follows this keyset cursor.
    pub after: Option<&'a Uri>,
    /// Maximum number of stored rows to return.
    pub limit: u32,
}

#[cfg(test)]
mod tests {
    use crate::{detail_level::ContextDetailLevel, list_query::ContextListQuery};

    #[test]
    fn query_can_request_an_unfiltered_summary_page() {
        let query = ContextListQuery {
            kind: None,
            detail_level: ContextDetailLevel::Summary,
            after: None,
            limit: 10,
        };
        pretty_assertions::assert_eq!(query.limit, 10);
    }
}
