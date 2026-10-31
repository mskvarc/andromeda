//! A concise plain value still collapses.

use andromeda_test_suite::{
    body_json::{json_absent, json_count, json_equals},
    broker,
    namespace::Namespace,
    outcome::Done,
    value::from_json,
    wire::request_to,
};
use googletest::prelude::*;
use http::{HeaderValue, Method, header::ACCEPT};
use ngsi_ld::model::Entity;
use serde_json::json;

/// Pins: Retrieve Entity with options=concise of Properties without sub-Attributes holding a number, an object and
/// an array answers each as its bare value
/// Clause: 4.5.2.3, 5.7.1.4
#[gtest]
#[tokio::test]
async fn n_ent_ret_9005_a_concise_plain_value_still_collapses() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let device_id = namespace.urn("Device", 1)?;
    let device_type = namespace.entity_type("Device")?;
    let device = Entity::builder()
        .id(device_id.clone())
        .entity_type(device_type)
        .property("count", 42)
        .property("settings", from_json(&json!({"a": 1})).or_fail()?)
        .property("tags", from_json(&json!(["a", "b"])).or_fail()?)
        .build()?;
    broker.entities().create(&device).send().await?.done()?;
    // The concise answer is read as written: the test pins each value bare, which the model reads into the same
    // Property as a wrapped one.
    let request =
        request_to(&broker, Method::GET, &format!("entities/{device_id}?options=concise"))?.append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let retrieved = broker.send_unchecked(request).await?;
    let body = retrieved.body();
    expect_that!(body, json_equals("$.count", 42));
    expect_that!(body, json_equals("$.settings.a", 1));
    expect_that!(body, json_absent("$.settings.value"));
    expect_that!(body, json_absent("$.settings.type"));
    expect_that!(body, json_count("$.tags", 2));
    expect_that!(body, json_equals("$.tags[0]", "a"));
    expect_that!(body, json_equals("$.tags[1]", "b"));
    Ok(())
}
