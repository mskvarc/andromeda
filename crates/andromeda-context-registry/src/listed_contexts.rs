//! Context collections returned by registry persistence.

use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::jsonld_context::ContextMetadata;
use serde::Serialize;

/// A Context list projected according to the requested detail level, written
/// as the JSON array clause 5.13.3 answers with.
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum ListedContexts {
    /// Full lifecycle metadata records.
    Detailed(Vec<ContextMetadata>),
    /// Context identifiers or public URLs.
    Summary(Vec<Identifier>),
}

#[cfg(test)]
mod tests {
    use crate::listed_contexts::ListedContexts;

    #[test]
    fn empty_summary_is_a_valid_list_result() {
        let listed = ListedContexts::Summary(Vec::new());
        assert!(matches!(listed, ListedContexts::Summary(values) if values.is_empty()));
    }
}
