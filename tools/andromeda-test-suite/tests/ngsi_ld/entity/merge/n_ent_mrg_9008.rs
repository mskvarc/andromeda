//! A merge deletes a null key inside a json property.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
    problem::answered,
    wire::request_to,
};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::CONTENT_TYPE};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, JsonProperty, NgsiLdMediaType, RawJson},
};
use serde_json::json;

/// Pins: Merge Entity setting one key of a `JsonProperty` json to urn:ngsi-ld:null answers 204 and removes that
/// key, keeping every other key, a JSON null among them
/// Clause: 5.5.12, 4.5.24.2, 4.5.0
#[gtest]
#[tokio::test]
async fn n_ent_mrg_9008_a_merge_deletes_a_null_key_inside_a_json_property() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_json_property_key_id = namespace.urn("Building", "json-property-key")?;
    let building_type = namespace.entity_type("Building")?;
    let json_text = format!(r#"{{"id": "some-id-{namespace}", "type": "SomeType_{namespace}", "null": null, "anythingElse": "anything-else"}}"#);
    let building_json_property_key = Entity::builder()
        .id(building_json_property_key_id.clone())
        .entity_type(building_type)
        .json_property("jsonProperty", JsonProperty::builder(json_text.parse::<RawJson>()?))
        .build()?;
    broker.entities().create(&building_json_property_key).send().await?.done()?;
    // The attribute builders refuse the NGSI-LD Null inside a json value, so the fragment is sent as written.
    let request = request_to(&broker, Method::PATCH, &format!("entities/{building_json_property_key_id}"))?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .body(Bytes::from(format!(
            "{{\"id\": \"{building_json_property_key_id}\", \"type\": \"Building_{namespace}\", \"jsonProperty\": {{\"type\": \"JsonProperty\", \"json\": {{\"anythingElse\": \"urn:ngsi-ld:null\"}}}}}}"
        )));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::NO_CONTENT));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_json_property_key_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(
        entity,
        has_attribute(
            "jsonProperty",
            any_attribute().json(json!({
                "id": format!("some-id-{namespace}"),
                "type": format!("SomeType_{namespace}"),
                "null": null
            }))
        )
    );
    Ok(())
}
