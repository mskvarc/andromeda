//! Batch upsert items spelled with keywords or user aliases are created.

use andromeda_test_suite::{
    attribute_matcher::{any_attribute, property},
    body_json::{json_count, json_includes},
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    wire::{content_type, request_to},
};
use bytes::Bytes;
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{APPLICATION_LD_JSON, NgsiLdMediaType},
};
use serde_json::json;

/// Pins: Batch Entity Upsert of one new item written with @id and @type and one written under its own @context
/// aliasing @type and `ngsi-ld:hasValue` answers 201 with both ids, and both Entities read back typed
/// Clause: 4.4, 5.5.7, 5.6.8
#[gtest]
#[tokio::test]
async fn n_bat_ups_9017_batch_upsert_items_spelled_with_keywords_or_user_aliases_are_created() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let keywords_id = namespace.urn("Building", "upsert-keywords")?;
    let aliased_id = namespace.urn("Building", "upsert-aliased")?;
    // Both items spell their keywords as `@id` and `@type` or alias them in a context of their own, which the model
    // cannot write, so the body is sent as written.
    let body = json!([
        {
            "@id": keywords_id.to_string(),
            "@type": format!("Building_{namespace}"),
            "height": {"@type": "Property", "value": 1},
            "@context": {}
        },
        {
            "id": aliased_id.to_string(),
            "kind": format!("Building_{namespace}"),
            "height": {"kind": "Property", "v": 2},
            "@context": {"kind": "@type", "v": "https://uri.etsi.org/ngsi-ld/hasValue"}
        }
    ]);
    let request = content_type(request_to(&broker, Method::POST, "entityOperations/upsert")?, &APPLICATION_LD_JSON)?.body(Bytes::from(body.to_string()));
    // The request is no typed operation, so the ids it created are read from the answer as received.
    let response = broker.send_unchecked(request).await?;
    expect_that!(response.status(), eq(StatusCode::CREATED));
    expect_that!(response.body(), json_count("$", 2));
    expect_that!(response.body(), json_includes("$", json!(keywords_id.to_string())));
    expect_that!(response.body(), json_includes("$", json!(aliased_id.to_string())));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&keywords_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("height", any_attribute().value(1)));
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&aliased_id)
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(entity_2, has_attribute("height", property().value(2)));
    Ok(())
}
