//! A concise array mixing an instance and a value is refused.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    problem::{answered, problem},
    value::object,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, Lossless, NgsiLdMediaType, Value},
};

/// Pins: Create Entity with an attribute written as an array holding a typeless instance and a bare number answers
/// 400 `BadRequestData` and stores nothing
/// Clause: 4.5.2.3, 4.5.5, 5.6.1.4, 5.5.4
/// Recorded: 4.5.2.3 reads an array of typeless objects holding value as instances and leaves an array mixing an
/// instance with a bare value unread; Andromeda refuses it as `BadRequestData`
#[gtest]
#[tokio::test]
async fn o_ent_cre_9002_a_concise_array_mixing_an_instance_and_a_value_is_refused() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let device_id = namespace.urn("Device", 1)?;
    let device_type = namespace.entity_type("Device")?;
    let device = Entity::builder()
        .id(device_id.clone())
        .entity_type(device_type)
        .property(
            "speed",
            Value::Array(vec![
                object([
                    ("value", Value::from(25)),
                    ("datasetId", Value::from(format!("urn:ngsi-ld:Dataset:{namespace}-a"))),
                ]),
                Value::from(30),
            ]),
        )
        .build()?;
    let answer = broker.with_responses().entities().create(&device).form(Lossless::Concise).send().await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&device_id)
        .send()
        .await;
    expect_that!(&answer_2, answered(StatusCode::NOT_FOUND));
    Ok(())
}
