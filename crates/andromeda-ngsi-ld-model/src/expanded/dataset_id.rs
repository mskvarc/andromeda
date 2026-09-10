//! Reading the `datasetId` that separates the instances of a multi-attribute.

use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_vocabulary::expanded::properties;
use serde_json::Value;

/// Extract the `datasetId` (`@id` value) from an expanded attribute instance.
///
/// `urn:ngsi-ld:null` is read as the identifier it literally is, not as a
/// deletion: clauses 5.5.8 and 5.5.12 state that "a datasetId cannot be deleted
/// by setting it to the value `urn:ngsi-ld:null`", so a payload carrying one is
/// refused before any write path reaches this function. Answering `None` for it
/// instead would retarget the default instance of clause 4.5.5, which is a
/// different attribute instance than the one the request named.
#[must_use]
pub fn get_instance_dataset_id(instance: &Value) -> Option<Uri> {
    instance
        .get(properties::DATASET_ID.as_str())
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|v| v.as_object())
        .and_then(|o| o.get("@id"))
        .and_then(|v| v.as_str())
        .and_then(|raw| raw.parse().ok())
}

/// Whether an expanded attribute instance carries a `datasetId`.
///
/// Presence-only sibling of [`get_instance_dataset_id`]: default-instance selection (clause 4.5.16.1) only needs to know
/// which instance lacks a `datasetId`, not what the others' identifiers are.
#[must_use]
pub fn instance_has_dataset_id(instance: &Value) -> bool {
    instance
        .get(properties::DATASET_ID.as_str())
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|v| v.as_object())
        .and_then(|o| o.get("@id"))
        .and_then(|v| v.as_str())
        .is_some()
}

#[cfg(test)]
mod tests {
    use crate::expanded::dataset_id::{get_instance_dataset_id, instance_has_dataset_id};
    use andromeda_ngsi_ld_lexicon::uri::Uri;
    use serde_json::json;

    #[test]
    fn extract_dataset_id() {
        let instance = json!({
            "https://uri.etsi.org/ngsi-ld/datasetId": [{"@id": "urn:ngsi-ld:Dataset:A"}]
        });
        pretty_assertions::assert_eq!(get_instance_dataset_id(&instance).as_ref().map(Uri::as_str), Some("urn:ngsi-ld:Dataset:A"));
    }

    #[test]
    fn presence_check_agrees_with_extraction() {
        let tagged = json!({"https://uri.etsi.org/ngsi-ld/datasetId": [{"@id": "urn:ngsi-ld:Dataset:A"}]});
        let untagged = json!({"https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 1}]});
        assert!(instance_has_dataset_id(&tagged));
        assert!(!instance_has_dataset_id(&untagged));
    }
}
