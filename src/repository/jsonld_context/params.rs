//! Repository input parameters for the JSON-LD context store.

use andromeda_ngsi_ld_lexicon::uri::Uri;

/// SQL projection shape selector — whether to fetch full metadata columns or
/// just identifier/url for listing and retrieval.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetailLevel {
    /// Return only identifiers or URLs.
    Summary,
    /// Return full metadata records.
    Full,
}

/// Keyset-paginated list parameters.
#[derive(Debug)]
pub struct ListContextsParams<'a> {
    /// Optional kind filter.
    pub kind: Option<&'a andromeda_domain::jsonld_context::ContextKind>,
    /// Projection shape (Summary vs Full).
    pub detail_level: DetailLevel,
    /// Keyset cursor — return rows with `id > after`.
    pub after: Option<&'a Uri>,
    /// Maximum rows to return.
    pub limit: u32,
}
