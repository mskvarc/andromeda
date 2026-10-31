//! A value holding nul is bad request data and creates nothing.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    problem::{answered, problem},
};
use googletest::prelude::*;
use http::StatusCode;
use indexmap::IndexMap;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, NgsiLdMediaType, Value},
};

/// Pins: Create Entity answers 400 `BadRequestData` and creates nothing when a Property value holds U+0000, as a string or inside a JSON object
/// Clause: 5.6.1, 5.5.4
/// Recorded: Andromeda limit, not v1.9.1: JSON admits U+0000 in a string and the specification sets no rule against it, but Andromeda cannot store the character, so it refuses the write as input it cannot take
#[gtest]
#[tokio::test]
async fn a_ent_cre_9056_a_value_holding_nul_is_bad_request_data_and_creates_nothing() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_string_id = namespace.urn("Vehicle", "string")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_object_id = namespace.urn("Vehicle", "object")?;
    let vehicle_string = Entity::builder()
        .id(vehicle_string_id.clone())
        .entity_type(vehicle_type.clone())
        .property("brandName", "nul\u{0}here")
        .build()?;
    let answer = broker.with_responses().entities().create(&vehicle_string).send().await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let vehicle_object = Entity::builder()
        .id(vehicle_object_id.clone())
        .entity_type(vehicle_type.clone())
        .property("brandName", Value::Object(IndexMap::from([("model".to_owned(), Value::from("nul\u{0}here"))])))
        .build()?;
    let answer_2 = broker.with_responses().entities().create(&vehicle_object).send().await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    for id in [&vehicle_string_id, &vehicle_object_id] {
        let answer_3 = broker
            .with_responses()
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .entities()
            .retrieve(id)
            .send()
            .await;
        expect_that!(&answer_3, answered(StatusCode::NOT_FOUND));
    }
    Ok(())
}
