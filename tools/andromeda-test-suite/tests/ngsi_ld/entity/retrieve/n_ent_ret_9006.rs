//! Every concise instance of a multi attribute keeps its object form.

use andromeda_test_suite::{
    attribute_matcher::property,
    body_json::{json_absent, json_count, json_equals},
    broker,
    entity_matcher::{every_instance, has_instances},
    namespace::Namespace,
    outcome::Done,
    problem::answered,
    wire::request_to,
    written_json::json_request_to,
};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, Property},
};

/// Pins: Retrieve Entity with options=concise of a Property with a dataset instance and a default instance answers
/// both as objects, the default one holding only its value, and the array written back reads back as two Property
/// instances
/// Clause: 4.5.2.3, 4.5.5, 5.7.1.4
#[gtest]
#[tokio::test]
async fn n_ent_ret_9006_every_concise_instance_of_a_multi_attribute_keeps_its_object_form() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let device_id = namespace.urn("Device", 1)?;
    let device_type = namespace.entity_type("Device")?;
    let device_back_id = namespace.urn("Device", "back")?;
    let device = Entity::builder()
        .id(device_id.clone())
        .entity_type(device_type)
        .property("speed", Property::builder(25).dataset_id(namespace.dataset_id("Dataset", "a")?))
        .property("speed", 30)
        .build()?;
    broker.entities().create(&device).send().await?.done()?;
    // The concise answer is read as written: the model cannot tell an instance holding only `value` from one that
    // also writes `type`, and the test pins that the default instance leaves `type` out.
    let request =
        request_to(&broker, Method::GET, &format!("entities/{device_id}?options=concise"))?.append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let retrieved = broker.send_unchecked(request).await?;
    let body = retrieved.body();
    expect_that!(body, json_count("$.speed", 2));
    expect_that!(body, json_count("$.speed[*].value", 2));
    expect_that!(body, json_equals("$.speed[?(@.value == 30)].value", 30));
    expect_that!(body, json_absent("$.speed[?(@.value == 30)].datasetId"));
    expect_that!(body, json_absent("$.speed[?(@.value == 30)].type"));
    expect_that!(
        body,
        json_equals("$.speed[?(@.value == 25)].datasetId", format!("urn:ngsi-ld:Dataset:{namespace}-a"))
    );
    // The concise instances carry no `type`, which the model writes, so the entity is sent as written.
    let request_2 = json_request_to(
        &broker,
        Method::POST,
        "entities",
        &format!(
            "{{\n    \"id\": \"{device_back_id}\",\n    \"type\": \"Device_{namespace}\",\n    \"speed\": [\n        {{ \"value\": 25, \"datasetId\": \"urn:ngsi-ld:Dataset:{namespace}-a\" }},\n        {{ \"value\": 30 }}\n    ]\n}}"
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
    expect_that!(entity, has_instances("speed", 2));
    expect_that!(entity, every_instance("speed", property()));
    Ok(())
}
