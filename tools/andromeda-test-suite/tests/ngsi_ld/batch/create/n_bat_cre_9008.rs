//! Batch items spelled with keywords or user aliases are created.

use andromeda_test_suite::{
    attribute_matcher::property,
    body_json::{json_count, json_includes},
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    problem::answered,
    wire::{content_type, request_to},
};
use bytes::Bytes;
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{APPLICATION_LD_JSON, NgsiLdMediaType, OneOrMany},
};

/// Pins: Batch Entity Creation of one item written with @id and @type and one written under its own @context
/// aliasing @type and ngsi-ld:hasValue answers 201 with both ids, and both Entities read back typed
/// Clause: 4.4, 5.5.7, 5.6.7
#[gtest]
#[tokio::test]
async fn n_bat_cre_9008_batch_items_spelled_with_keywords_or_user_aliases_are_created() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_batch_keywords_id = namespace.urn("Building", "batch-keywords")?;
    let building_type = namespace.entity_type("Building")?;
    let building_batch_aliased_id = namespace.urn("Building", "batch-aliased")?;
    // Each item carries an `@context` of its own, which the client does not write into a batch, so the body is sent as written.
    let body = format!(
        r#"[{{"@id": "urn:ngsi-ld:Building:{namespace}-batch-keywords", "@type": "Building_{namespace}", "height": {{"@type": "Property", "value": 1}}, "@context": {{}}}}, {{"id": "urn:ngsi-ld:Building:{namespace}-batch-aliased", "kind": "Building_{namespace}", "height": {{"kind": "Property", "v": 2}}, "@context": {{"kind": "@type", "v": "https://uri.etsi.org/ngsi-ld/hasValue"}}}}]"#
    );
    let request = content_type(request_to(&broker, Method::POST, "entityOperations/create")?, &APPLICATION_LD_JSON)?.body(Bytes::from(body));
    // The request is no typed operation, so the ids it created are read from the answer as received.
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let received = answer?;
    let created = received.body();
    expect_that!(created, json_count("$", 2));
    expect_that!(created, json_includes("$", building_batch_keywords_id.to_string()));
    expect_that!(created, json_includes("$", building_batch_aliased_id.to_string()));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_batch_keywords_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity.entity_type, eq(&OneOrMany::one(building_type.clone())));
    expect_that!(entity, has_attribute("height", property().value(1)));
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_batch_aliased_id)
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(entity_2.entity_type, eq(&OneOrMany::one(building_type)));
    expect_that!(entity_2, has_attribute("height", property().value(2)));
    Ok(())
}
