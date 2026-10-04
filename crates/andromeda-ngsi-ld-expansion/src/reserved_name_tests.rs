//! A term the core @context defines for an attribute's own members never names
//! an attribute (clause 4.5.1), whichever expansion path reads the write.

use crate::{
    expansion_fixture::{engine, entity, fragment, patch},
    normalization::Normalization,
};
use serde_json::json;

/// The refusal both expansions give a write naming an attribute `value`,
/// which the protected core @context expands to `ngsi-ld:hasValue` (clause
/// 5.5.7).
const VALUE_REFUSAL: &str =
    "'value' (https://uri.etsi.org/ngsi-ld/hasValue) is a member the NGSI-LD core @context defines for itself and cannot be used as an attribute name";

#[tokio::test]
async fn an_entity_attribute_named_value_is_refused_on_both_paths() {
    let refusal = entity(
        &engine(),
        json!({"id": "urn:ngsi-ld:Vehicle:r", "type": "Vehicle", "value": {"type": "Property", "value": 1}}),
        Normalization::Entity,
    )
    .await
    .expect("the fixture expands")
    .refused();
    pretty_assertions::assert_eq!(refusal, VALUE_REFUSAL);
}

#[tokio::test]
async fn an_append_fragment_attribute_named_value_is_refused_on_both_paths() {
    let refusal = fragment(&engine(), json!({"value": {"type": "Property", "value": 1}}), Normalization::Entity)
        .await
        .expect("the fixture expands")
        .refused();
    pretty_assertions::assert_eq!(refusal, VALUE_REFUSAL);
}

#[tokio::test]
async fn a_merge_patch_attribute_named_dataset_id_is_refused_on_both_paths() {
    let refusal = patch(
        &engine(),
        json!({"datasetId": {"type": "Property", "value": 1}}),
        Normalization::MemberDeletions,
    )
    .await
    .expect("the fixture expands")
    .refused();
    pretty_assertions::assert_eq!(
        refusal,
        "'datasetId' (https://uri.etsi.org/ngsi-ld/datasetId) is a member the NGSI-LD core @context defines for itself and cannot be used as an attribute name"
    );
}

#[tokio::test]
async fn a_client_term_beside_the_reserved_ones_still_names_an_attribute() {
    let document = entity(
        &engine(),
        json!({"id": "urn:ngsi-ld:Vehicle:r", "type": "Vehicle", "reading": {"type": "Property", "value": 1}}),
        Normalization::Entity,
    )
    .await
    .expect("the fixture expands")
    .agreed();
    assert!(document.attributes().contains_key("https://uri.etsi.org/ngsi-ld/default-context/reading"));
}
