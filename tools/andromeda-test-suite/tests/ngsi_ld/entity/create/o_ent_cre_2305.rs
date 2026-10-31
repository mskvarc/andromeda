//! Numbers beyond the numeric range in a sub attribute an array and a value object are refused naming the limit.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    problem::{answered, problem},
};
use bytes::Bytes;
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::CreateEntity},
    model::{Entity, ErrorType, NgsiLdMediaType},
};

/// The body of the first request.
const BODY_1: &str = r#"{"id": "urn:ngsi-ld:Vehicle:{{ns}}-1", "type": "Vehicle_{{ns}}", "speed": {"type": "Property", "value": 1, "accuracy": {"type": "Property", "value": 1e200000}}, "samples": {"type": "Property", "value": [1e-16384, 2]}, "reading": {"type": "Property", "value": {"huge": 1e200000}}}"#;

/// The body of the second request.
const BODY_2: &str = r#"{"id": "urn:ngsi-ld:Vehicle:{{ns}}-1", "type": "Vehicle_{{ns}}", "samples": {"type": "Property", "value": [1e-16384, 2]}}"#;

/// The body of the third request.
const BODY_3: &str = r#"{"id": "urn:ngsi-ld:Vehicle:{{ns}}-1", "type": "Vehicle_{{ns}}", "reading": {"type": "Property", "value": {"huge": 1e200000}}}"#;

/// Pins: Create Entity refuses a number beyond the numeric range inside a sub-attribute, an array value and a
/// value object with 400 `BadRequestData` naming the broken limit and the JSON Pointer of the number, and creates no
/// entity
/// Clause: 4.5.1, 5.6.1
/// Recorded: IETF RFC 8259 clause 6 lets an implementation limit the range and precision of numbers and ETSI GS CIM 009
/// v1.9.1 sets none; Andromeda stores numbers of at most 131072 digits before and 16383 after the decimal point
/// (`PostgreSQL` numeric) and refuses others naming the broken limit
#[gtest]
#[tokio::test]
async fn o_ent_cre_2305_numbers_beyond_the_numeric_range_in_a_sub_attribute_an_array_and_a_value_object_are_refused_naming_the_limit() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle = Entity::builder().id(vehicle_id.clone()).entity_type(vehicle_type).build()?;
    // The model cannot hold a number outside the range of a double, so the bodies are sent as written.
    let request = broker
        .unchecked(&CreateEntity::builder().entity(&vehicle).build())?
        .body(Bytes::from(BODY_1.replace("{{ns}}", namespace.token())));
    let answer = broker.send_unchecked(request).await;
    expect_that!(
        &answer,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail("the JSON number at /speed/accuracy/value is refused: the number needs more than 131072 digits before the decimal point; Andromeda stores numbers of at most 131072 digits before and 16383 digits after it")
    );
    let request_2 = broker
        .unchecked(&CreateEntity::builder().entity(&vehicle).build())?
        .body(Bytes::from(BODY_2.replace("{{ns}}", namespace.token())));
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(
        &answer_2,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail("the JSON number at /samples/value/0 is refused: the number needs more than 16383 digits after the decimal point; Andromeda stores numbers of at most 131072 digits before and 16383 digits after it")
    );
    let request_3 = broker
        .unchecked(&CreateEntity::builder().entity(&vehicle).build())?
        .body(Bytes::from(BODY_3.replace("{{ns}}", namespace.token())));
    let answer_3 = broker.send_unchecked(request_3).await;
    expect_that!(
        &answer_3,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail("the JSON number at /reading/value/huge is refused: the number needs more than 131072 digits before the decimal point; Andromeda stores numbers of at most 131072 digits before and 16383 digits after it")
    );
    let answer_4 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await;
    expect_that!(&answer_4, answered(StatusCode::NOT_FOUND));
    Ok(())
}
