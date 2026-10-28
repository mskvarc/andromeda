//! ETSI `056_10`: keys inside a `JsonProperty` `json` object are removed when a Merge Entity sets them to the NGSI-LD Null
//! (clauses 5.6.17, 6.5.3.4 and 4.5.24).

use crate::{
    suite_keywords::{create_entity, delete_entities, merge_entity, retrieve_entity_json},
    suite_payloads::building_with_json_property,
};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace, vocabulary::Vocabulary};
use googletest::prelude::*;
use ngsi_ld::{
    http::ContextDelivery,
    model::{EntityFragment, JsonProperty, NgsiLdMediaType},
};
use serde_json::json;

/// The terms of the `@context` the cases register.
const TERMS: [&str; 2] = ["Building", "jsonProperty"];

/// The `json` of the merge: the member `anythingElse` set to the NGSI-LD Null.
const JSON_MERGE: &str = r#"{"anythingElse":"urn:ngsi-ld:null"}"#;

/// Pins: Check that keys inside a `JsonProperty` json object are removed when set to NGSI-LD Null
/// Clause: 5.6.17, 6.5.3.4, 4.5.24
/// Changed: the body is compared as one exact document, where the Hurl file checks its members one by one
#[gtest]
#[tokio::test]
async fn e056_10_01_remove_key_from_json_property_object() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    let original = building_with_json_property(&namespace, 1)?;
    create_entity(&broker, &vocabulary, &original).await?;
    let fragment = EntityFragment::for_entity(namespace.urn("Building", 1)?, "Building")
        .json_property("jsonProperty", JsonProperty::builder(JSON_MERGE.parse()?))
        .build()?;
    let client = broker.with_context(vocabulary.context())?.with_context_delivery(ContextDelivery::LdJsonBody);
    merge_entity(&client, &original.id, &fragment).await?;
    let client = broker.with_context(vocabulary.context())?.with_accept(NgsiLdMediaType::JsonLd);
    let body = retrieve_entity_json(&client, &original.id).await?;
    expect_that!(
        &body,
        matches_document(json!({
            "id": namespace.urn_text("Building", 1),
            "type": "Building",
            "jsonProperty": {
                "type": "JsonProperty",
                "json": {"id": "some-id", "type": "some-type", "null": null},
            },
            "@context": vocabulary.url().as_str(),
        }))
        .hiding("instanceId")
    );
    delete_entities(&broker, &[&original.id]).await;
    Ok(())
}
