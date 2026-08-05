//! Reading the `datasetId` that separates the instances of a multi-attribute.

use andromeda_ngsi_ld_vocabulary::expanded::properties;
use serde_json::Value;

/// Extract the `datasetId` (`@id` value) from an expanded attribute instance.
pub fn get_instance_dataset_id(instance: &Value) -> Option<String> {
    instance
        .get(properties::DATASET_ID.as_str())
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|v| v.as_object())
        .and_then(|o| o.get("@id"))
        .and_then(|v| v.as_str())
        .map(String::from)
}

#[cfg(test)]
mod tests {
    use crate::expanded::dataset_id::get_instance_dataset_id;
    use serde_json::json;

    #[test]
    fn extract_dataset_id() {
        let instance = json!({
            "https://uri.etsi.org/ngsi-ld/datasetId": [{"@id": "urn:ngsi-ld:Dataset:A"}]
        });
        pretty_assertions::assert_eq!(get_instance_dataset_id(&instance).as_deref(), Some("urn:ngsi-ld:Dataset:A"));
    }
}
