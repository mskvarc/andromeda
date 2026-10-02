//! The named choices that steer a write: what it overwrites and which
//! instances of a multi-attribute it touches.

use crate::document::instance_document::InstanceDocument;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;

/// Whether an append replaces an attribute instance that already exists
/// (clause 5.6.3, `noOverwrite` option).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OverwritePolicy {
    /// Existing instances are replaced by the incoming ones.
    Overwrite,
    /// Existing instances are kept and the incoming ones reported as not
    /// appended.
    Preserve,
}

/// Which instances of a multi-attribute an operation targets (clause 5.6.5).
///
/// One parameter rather than a `datasetId` plus a `deleteAll` flag, so
/// "delete everything, but only dataset A" cannot be expressed at all.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DatasetSelection<'a> {
    /// Every instance, regardless of `datasetId`.
    All,
    /// The single instance carrying this `datasetId`.
    Single(&'a Identifier),
    /// The default instance, the one with no `datasetId`.
    Default,
}

/// The `datasetId` of the one instance an Attribute Fragment addresses
/// through its URL (clauses 5.6.4 and 5.6.19).
///
/// Clauses 5.6.4.4 and 5.6.19.4 target the instance "with the specified
/// datasetId if present", otherwise the default instance of clause 4.5.5.
/// Tables 6.7.3.1-1 and 6.7.3.3-1 give neither operation a `datasetId` URL
/// parameter, so the `datasetId` they match on is the Fragment's own; the
/// parameter Andromeda additionally accepts selects ahead of it when a client
/// sends one.
#[must_use]
pub fn addressed_dataset<'a>(url_dataset_id: Option<&'a Identifier>, fragment: &'a InstanceDocument) -> Option<&'a Identifier> {
    url_dataset_id.or_else(|| fragment.dataset_id())
}

#[cfg(test)]
mod tests {
    use crate::{document::instance_document::InstanceDocument, operation::selection::addressed_dataset};
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;

    fn dataset(raw: &str) -> Identifier {
        raw.parse().expect("a valid dataset URI")
    }

    fn fragment(dataset_id: Option<&str>) -> InstanceDocument {
        InstanceDocument::builder().dataset_id(dataset_id.map(dataset)).build()
    }

    #[test]
    fn the_url_dataset_id_addresses_ahead_of_the_fragments_own() {
        let url = dataset("urn:ngsi-ld:Dataset:url");
        let fragment = fragment(Some("urn:ngsi-ld:Dataset:fragment"));
        pretty_assertions::assert_eq!(addressed_dataset(Some(&url), &fragment), Some(&url));
    }

    #[test]
    fn without_a_url_dataset_id_the_fragments_own_addresses_the_instance() {
        let fragment = fragment(Some("urn:ngsi-ld:Dataset:fragment"));
        pretty_assertions::assert_eq!(addressed_dataset(None, &fragment), Some(&dataset("urn:ngsi-ld:Dataset:fragment")));
    }

    #[test]
    fn a_fragment_naming_no_dataset_id_addresses_the_default_instance() {
        pretty_assertions::assert_eq!(addressed_dataset(None, &fragment(None)), None);
    }
}
