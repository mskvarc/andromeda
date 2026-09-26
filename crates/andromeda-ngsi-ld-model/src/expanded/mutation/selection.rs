//! The named choices that steer a mutation: what it overwrites and which
//! instances of a multi-attribute it touches.

use crate::expanded::dataset_id::{InstanceDatasetIdError, get_instance_dataset_id};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use serde_json::Value;

/// Whether an append replaces an attribute instance that already exists
/// (NGSI-LD clause 5.6.3, `noOverwrite` option).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverwritePolicy {
    /// Existing instances are replaced by the incoming ones.
    Overwrite,
    /// Existing instances are kept and the incoming ones reported as not appended.
    Preserve,
}

/// Which instances of a multi-attribute an operation targets
/// (NGSI-LD clause 5.6.5).
///
/// One parameter rather than a `datasetId` plus a `deleteAll` flag, so
/// "delete everything, but only dataset A" cannot be expressed at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DatasetSelection<'a> {
    /// Every instance, regardless of `datasetId`.
    All,
    /// The single instance carrying this `datasetId`.
    Single(&'a Uri),
    /// The default instance, the one with no `datasetId`.
    Default,
}

/// The `datasetId` of the one instance an Attribute Fragment addressed through
/// its URL (clauses 5.6.4 and 5.6.19).
///
/// Clauses 5.6.4.4 and 5.6.19.4 target the instance "with the specified
/// datasetId if present", otherwise the default instance of clause 4.5.5.
/// Tables 6.7.3.1-1 and 6.7.3.3-1 give neither operation a `datasetId` URL
/// parameter, so the `datasetId` they match on is the Fragment's own; the
/// parameter Andromeda additionally accepts selects ahead of it when a client
/// sends one. The Fragment's own is read even then, so an unreadable one is
/// refused whichever of the two decides.
///
/// # Errors
///
/// Returns [`InstanceDatasetIdError`] when the Fragment carries a `datasetId`
/// that is not an identifier.
pub fn addressed_dataset(url_dataset_id: Option<&Uri>, fragment: &Value) -> Result<Option<Uri>, InstanceDatasetIdError> {
    let fragment_dataset_id = get_instance_dataset_id(fragment)?;
    Ok(url_dataset_id.cloned().or(fragment_dataset_id)) // clone: the addressed identity outlives the borrowed URL parameter.
}

#[cfg(test)]
mod tests {
    use crate::expanded::mutation::selection::addressed_dataset;
    use andromeda_ngsi_ld_lexicon::uri::Uri;
    use serde_json::json;

    fn dataset(raw: &str) -> Uri {
        raw.parse().expect("a valid dataset URI")
    }

    #[test]
    fn the_url_dataset_id_addresses_ahead_of_the_fragments_own() {
        let fragment = json!({"https://uri.etsi.org/ngsi-ld/datasetId": [{"@id": "urn:ngsi-ld:Dataset:fragment"}]});
        pretty_assertions::assert_eq!(
            addressed_dataset(Some(&dataset("urn:ngsi-ld:Dataset:url")), &fragment),
            Ok(Some(dataset("urn:ngsi-ld:Dataset:url")))
        );
    }

    #[test]
    fn without_a_url_dataset_id_the_fragments_own_addresses_the_instance() {
        let fragment = json!({"https://uri.etsi.org/ngsi-ld/datasetId": [{"@id": "urn:ngsi-ld:Dataset:fragment"}]});
        pretty_assertions::assert_eq!(addressed_dataset(None, &fragment), Ok(Some(dataset("urn:ngsi-ld:Dataset:fragment"))));
    }

    #[test]
    fn a_fragment_naming_no_dataset_id_addresses_the_default_instance() {
        pretty_assertions::assert_eq!(addressed_dataset(None, &json!({})), Ok(None));
    }

    #[test]
    fn an_unreadable_fragment_dataset_id_is_refused_even_when_the_url_one_addresses() {
        let fragment = json!({"https://uri.etsi.org/ngsi-ld/datasetId": [{"@id": "not a uri"}]});
        assert!(addressed_dataset(Some(&dataset("urn:ngsi-ld:Dataset:url")), &fragment).is_err());
    }
}
