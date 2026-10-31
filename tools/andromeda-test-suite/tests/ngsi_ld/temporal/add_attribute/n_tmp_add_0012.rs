//! A fragment naming another entity after a core delete restores nothing.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    outcome::{Done, Upserted},
    problem::{answered, problem},
    wire::request_to,
};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::CONTENT_TYPE};
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType, Property, evolution::TemporalEntity},
};

/// Pins: after the Entity is deleted through the Core API, Add Attributes whose Fragment id names another Entity is
/// refused with 400 `BadRequestData`, and neither the path's Entity nor the named one has current state afterwards
/// Clause: 5.6.12.4, 6.20.3.1, 5.4, 5.5.4, 5.6.6.4
#[gtest]
#[tokio::test]
async fn n_tmp_add_0012_a_fragment_naming_another_entity_after_a_core_delete_restores_nothing() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_2_id = namespace.urn("Vehicle", 2)?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("speed", Property::builder(120).observed_at("2020-09-01T12:03:00Z".parse()?))
        .property("speed", Property::builder(80).observed_at("2020-09-01T12:05:00Z".parse()?))
        .property("fuelLevel", Property::builder(67).observed_at("2020-09-01T12:03:00Z".parse()?))
        .property("fuelLevel", Property::builder(53).observed_at("2020-09-01T13:05:00Z".parse()?))
        .property(
            "fuelLevel",
            Property::builder(40)
                .dataset_id(namespace.dataset_id("Dataset", "fuel")?)
                .observed_at("2020-09-01T14:07:00Z".parse()?),
        )
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
    broker.entities().delete(&vehicle_id).send().await?.done()?;
    // The temporal attribute addition names its own target in the fragment, so a fragment naming another entity is sent as written.
    let request = request_to(&broker, Method::POST, &format!("temporal/entities/{vehicle_id}/attrs"))?.append_header(CONTENT_TYPE, HeaderValue::from_static("application/json")).body(Bytes::from(format!("{{\n    \"id\": \"urn:ngsi-ld:Vehicle:{namespace}-2\",\n    \"type\": \"Vehicle_{namespace}\",\n    \"speed\": [\n        {{ \"type\": \"Property\", \"value\": 99, \"observedAt\": \"2020-09-02T12:00:00Z\" }}\n    ]\n}}")));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await;
    expect_that!(&answer_2, answered(StatusCode::NOT_FOUND));
    let answer_3 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_2_id)
        .send()
        .await;
    expect_that!(&answer_3, answered(StatusCode::NOT_FOUND));
    Ok(())
}
