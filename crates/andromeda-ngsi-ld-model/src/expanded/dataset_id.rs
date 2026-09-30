//! Reading the `datasetId` that separates the instances of a multi-attribute.

use andromeda_ngsi_ld_lexicon::identifier::{Identifier, IdentifierParseError};
use andromeda_ngsi_ld_vocabulary::expanded::properties;
use serde_json::Value;
use thiserror::Error;

/// An attribute instance carries a `datasetId` that is not an identifier.
///
/// Clause 4.5.5 makes the `datasetId` part of an instance's identity, so an
/// unreadable one cannot be read as absent: that would retarget the default
/// instance, a different instance than the one the payload named.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
#[error("the datasetId is not a valid identifier: {source}")]
pub struct InstanceDatasetIdError {
    /// Why it is not an identifier.
    #[source]
    pub source: IdentifierParseError,
}

/// The `@id` spelling of an expanded instance's `datasetId`, when it has one.
fn dataset_id_spelling(instance: &Value) -> Option<&str> {
    instance
        .get(properties::DATASET_ID.as_str())
        .and_then(Value::as_array)
        .and_then(|arr| arr.first())
        .and_then(Value::as_object)
        .and_then(|o| o.get("@id"))
        .and_then(Value::as_str)
}

/// Extract the `datasetId` (`@id` value) from an expanded attribute instance.
///
/// `Ok(None)` means the instance carries no `datasetId`, and so is the default
/// instance of clause 4.5.5. A `datasetId` that is present but is not an
/// identifier is an error, never `None`.
///
/// `urn:ngsi-ld:null` is read as the identifier it literally is, not as a
/// deletion: clauses 5.5.8 and 5.5.12 state that "a datasetId cannot be deleted
/// by setting it to the value `urn:ngsi-ld:null`", so a payload carrying one is
/// refused before any write path reaches this function. Answering `None` for it
/// instead would retarget the default instance of clause 4.5.5, which is a
/// different attribute instance than the one the request named.
///
/// # Errors
///
/// Returns [`InstanceDatasetIdError`] when the `datasetId` is present but is not
/// a valid identifier, including one past the identifier bound.
pub fn get_instance_dataset_id(instance: &Value) -> Result<Option<Identifier>, InstanceDatasetIdError> {
    let Some(raw) = dataset_id_spelling(instance) else {
        return Ok(None);
    };
    raw.parse().map(Some).map_err(|source| InstanceDatasetIdError { source })
}

/// The position of the first instance whose `datasetId` is `dataset`, where
/// `None` selects the default instance (clause 4.5.5).
///
/// # Errors
///
/// Returns [`InstanceDatasetIdError`] for the first instance, before any match,
/// whose `datasetId` is not a valid identifier.
pub fn position_of_dataset(instances: &[Value], dataset: Option<&Identifier>) -> Result<Option<usize>, InstanceDatasetIdError> {
    instances
        .iter()
        .enumerate()
        .find_map(|(position, instance)| match get_instance_dataset_id(instance) {
            Ok(candidate) => (candidate.as_ref() == dataset).then_some(Ok(position)),
            Err(error) => Some(Err(error)),
        })
        .transpose()
}

/// The `datasetId` of every instance, in order.
///
/// # Errors
///
/// Returns [`InstanceDatasetIdError`] for the first instance whose `datasetId`
/// is not a valid identifier.
pub fn instance_dataset_ids(instances: &[Value]) -> Result<Vec<Option<Identifier>>, InstanceDatasetIdError> {
    instances.iter().map(get_instance_dataset_id).collect()
}

/// Whether an expanded attribute instance carries a `datasetId`.
///
/// Presence-only sibling of [`get_instance_dataset_id`]: default-instance selection (clause 4.5.16.1) only needs to know
/// which instance lacks a `datasetId`, not what the others' identifiers are.
#[must_use]
pub fn instance_has_dataset_id(instance: &Value) -> bool {
    dataset_id_spelling(instance).is_some()
}

#[cfg(test)]
mod tests {
    use crate::expanded::dataset_id::{get_instance_dataset_id, instance_dataset_ids, instance_has_dataset_id, position_of_dataset};
    use andromeda_ngsi_ld_lexicon::{
        identifier::{Identifier, IdentifierParseError},
        identifier_length::IDENTIFIER_BYTE_LIMIT,
    };
    use serde_json::{Value, json};

    /// An instance whose `datasetId` is `raw`.
    fn tagged(raw: &str) -> Value {
        json!({"https://uri.etsi.org/ngsi-ld/datasetId": [{"@id": raw}]})
    }

    #[test]
    fn extract_dataset_id() {
        let instance = json!({
            "https://uri.etsi.org/ngsi-ld/datasetId": [{"@id": "urn:ngsi-ld:Dataset:A"}]
        });
        pretty_assertions::assert_eq!(
            get_instance_dataset_id(&instance)
                .expect("a readable datasetId")
                .as_ref()
                .map(Identifier::as_str),
            Some("urn:ngsi-ld:Dataset:A")
        );
    }

    #[test]
    fn presence_check_agrees_with_extraction() {
        let tagged = json!({"https://uri.etsi.org/ngsi-ld/datasetId": [{"@id": "urn:ngsi-ld:Dataset:A"}]});
        let untagged = json!({"https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 1}]});
        assert!(instance_has_dataset_id(&tagged));
        assert!(!instance_has_dataset_id(&untagged));
    }

    #[test]
    fn an_over_long_dataset_id_is_an_error_rather_than_the_default_instance() {
        let raw = format!("urn:ngsi-ld:Dataset:{}", "a".repeat(IDENTIFIER_BYTE_LIMIT));
        let error = get_instance_dataset_id(&tagged(&raw)).expect_err("an over-long datasetId");
        assert!(matches!(error.source, IdentifierParseError::TooLong(_)), "{error:?}");
        assert!(instance_has_dataset_id(&tagged(&raw)), "the instance still names a dataset");
    }

    #[test]
    fn an_absent_dataset_id_is_the_default_instance() {
        let untagged = json!({"https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 1}]});
        pretty_assertions::assert_eq!(get_instance_dataset_id(&untagged), Ok(None));
    }

    #[test]
    fn locating_a_dataset_stops_at_an_unreadable_dataset_id_before_any_match() {
        let default = json!({"https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 1}]});
        let instances = [tagged("not a uri"), default];
        assert!(position_of_dataset(&instances, None).is_err());
        assert!(instance_dataset_ids(&instances).is_err());

        let readable = [tagged("urn:ngsi-ld:Dataset:A"), json!({})];
        pretty_assertions::assert_eq!(position_of_dataset(&readable, None), Ok(Some(1)));
    }
}
