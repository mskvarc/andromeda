//! Context collections returned by registry persistence.

use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::jsonld_context::ContextMetadata;

/// A Context list projected according to the requested detail level.
#[derive(Debug)]
pub enum ListedContexts {
    /// Full lifecycle metadata records.
    Detailed(Vec<ContextMetadata>),
    /// Context identifiers or public URLs.
    Summary(Vec<Uri>),
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
