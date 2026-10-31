//! A concise value that would read as an attribute keeps its value member.

use andromeda_test_suite::{
    attribute_matcher::property,
    body_json::{json_absent, json_count, json_equals},
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
    problem::answered,
    value::from_json,
    wire::request_to,
    written_json::json_request_to,
};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
};
use serde_json::json;

/// Pins: Retrieve Entity with options=concise of Properties whose values are an object with a type, an object with
/// an object member, an object with a value member and an array of objects with object members answers each as an
/// object holding only value, and those concise attributes written back read back normalized as the same Properties
/// Clause: 4.5.2.3, 5.2.5, 5.7.1.4
#[gtest]
#[tokio::test]
async fn n_ent_ret_9004_a_concise_value_that_would_read_as_an_attribute_keeps_its_value_member() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let device_id = namespace.urn("Device", 1)?;
    let device_type = namespace.entity_type("Device")?;
    let device_back_id = namespace.urn("Device", "back")?;
    let thing = format!("urn:ngsi-ld:Thing:{namespace}-1");
    let thing_2 = format!("urn:ngsi-ld:Thing:{namespace}-2");
    let feature = from_json(&json!({"type": "Feature", "x": 1})).or_fail()?;
    let pointer = from_json(&json!({"object": thing})).or_fail()?;
    let nested = from_json(&json!({"value": 1, "unit": "x"})).or_fail()?;
    let pointers = from_json(&json!([{"object": thing}, {"object": thing_2}])).or_fail()?;
    let device = Entity::builder()
        .id(device_id.clone())
        .entity_type(device_type)
        .property("feature", feature.clone())
        .property("pointer", pointer.clone())
        .property("nested", nested.clone())
        .property("pointers", pointers.clone())
        .build()?;
    broker.entities().create(&device).send().await?.done()?;
    // The concise answer is read as written: the model cannot tell an attribute holding only `value` from one that
    // also writes `type`, and the test pins that the concise form leaves `type` out.
    let request =
        request_to(&broker, Method::GET, &format!("entities/{device_id}?options=concise"))?.append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let retrieved = broker.send_unchecked(request).await?;
    let body = retrieved.body();
    expect_that!(body, json_equals("$.feature.value.type", "Feature"));
    expect_that!(body, json_equals("$.feature.value.x", 1));
    expect_that!(body, json_equals("$.pointer.value.object", thing.as_str()));
    expect_that!(body, json_equals("$.nested.value.value", 1));
    expect_that!(body, json_equals("$.nested.value.unit", "x"));
    expect_that!(body, json_count("$.pointers.value", 2));
    expect_that!(body, json_equals("$.pointers.value[0].object", thing.as_str()));
    expect_that!(body, json_equals("$.pointers.value[1].object", thing_2.as_str()));
    expect_that!(body, json_absent("$.feature.type"));
    expect_that!(body, json_absent("$.pointer.type"));
    expect_that!(body, json_absent("$.nested.type"));
    expect_that!(body, json_absent("$.pointers.type"));
    expect_that!(body, json_absent("$.nested.unit"));
    expect_that!(body, json_absent("$.pointer.object"));
    // The concise form of an attribute holding only a value is the bare value, which the model writes instead of the
    // `value` member, so the entity is sent as written.
    let request_2 = json_request_to(
        &broker,
        Method::POST,
        "entities",
        &format!(
            "{{\n    \"id\": \"{device_back_id}\",\n    \"type\": \"Device_{namespace}\",\n    \"feature\": {{ \"value\": {{ \"type\": \"Feature\", \"x\": 1 }} }},\n    \"pointer\": {{ \"value\": {{ \"object\": \"{thing}\" }} }},\n    \"nested\": {{ \"value\": {{ \"value\": 1, \"unit\": \"x\" }} }},\n    \"pointers\": {{ \"value\": [ {{ \"object\": \"{thing}\" }}, {{ \"object\": \"{thing_2}\" }} ] }}\n}}"
        ),
    )?;
    let answer = broker.send_unchecked(request_2).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let retrieved_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&device_back_id)
        .send()
        .await?;
    let entity = retrieved_3.body();
    expect_that!(entity, has_attribute("feature", property().value(feature)));
    expect_that!(entity, has_attribute("pointer", property().value(pointer)));
    expect_that!(entity, has_attribute("nested", property().value(nested)));
    expect_that!(entity, has_attribute("pointers", property().value(pointers)));
    Ok(())
}
