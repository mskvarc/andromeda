//! Numbers beyond the numeric range are refused naming the limit.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::{has_attribute, lacks_attribute},
    namespace::Namespace,
    outcome::Done,
    problem::problem,
    wire::request_to,
};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::CONTENT_TYPE};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, NgsiLdMediaType},
};

/// Pins: Replace Entity refuses numbers beyond the numeric range, both a very large and a very small exponent, with
/// 400 `BadRequestData` naming the broken limit and leaves the entity unchanged
/// Clause: 4.5.1, 5.6.18
/// Recorded: IETF RFC 8259 clause 6 lets an implementation limit the range and precision of numbers and ETSI GS CIM 009
/// v1.9.1 sets none; Andromeda stores numbers of at most 131072 digits before and 16383 after the decimal point
/// (`PostgreSQL` numeric) and refuses others naming the broken limit
#[gtest]
#[tokio::test]
async fn o_ent_rpl_2303_numbers_beyond_the_numeric_range_are_refused_naming_the_limit() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle = Entity::builder().id(vehicle_id.clone()).entity_type(vehicle_type).property("big", 1).build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    // The model refuses this entity, so the replacement is sent as written.
    let request = request_to(&broker, Method::PUT, &format!("entities/{vehicle_id}"))?.append_header(CONTENT_TYPE, HeaderValue::from_static("application/json")).body(Bytes::from(format!("{{\"type\": \"Vehicle_{namespace}\", \"big\": {{\"type\": \"Property\", \"value\": 1e200000}}, \"tiny\": {{\"type\": \"Property\", \"value\": 1e-16384}}}}")));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail("the JSON number at /big/value is refused: the number needs more than 131072 digits before the decimal point; Andromeda stores numbers of at most 131072 digits before and 16383 digits after it"));
    // The model refuses this entity, so the replacement is sent as written.
    let request_2 = request_to(&broker, Method::PUT, &format!("entities/{vehicle_id}"))?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .body(Bytes::from(format!(
            "{{\"type\": \"Vehicle_{namespace}\", \"tiny\": {{\"type\": \"Property\", \"value\": 1e-16384}}}}"
        )));
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail("the JSON number at /tiny/value is refused: the number needs more than 16383 digits after the decimal point; Andromeda stores numbers of at most 131072 digits before and 16383 digits after it"));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, lacks_attribute("tiny"));
    expect_that!(entity, has_attribute("big", any_attribute().value(1)));
    Ok(())
}
