//! A number beyond the numeric range is refused and creates nothing.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::CreateEntity},
    model::{Entity, ErrorType, Lossless, NgsiLdMediaType, RawJson},
};

/// Pins: Create Entity whose Property reading holds 1e-16384 answers 400 `BadRequestData`, and the Entity does not
/// exist
/// Clause: 4.5.1, 5.6.1
/// Recorded: IETF RFC 8259 clause 6 lets an implementation limit the range and precision of numbers and ETSI GS CIM
/// 009 v1.9.1 sets none; Andromeda stores numbers of at most 131072 digits before and 16383 after the decimal point
/// (`PostgreSQL` numeric) and refuses others
#[gtest]
#[tokio::test]
async fn o_ent_cre_9035_a_number_beyond_the_numeric_range_is_refused_and_creates_nothing() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_out_of_range_number_id = namespace.urn("Building", "out-of-range-number")?;
    let building_type = namespace.entity_type("Building")?;
    let building_out_of_range_number = Entity::builder()
        .id(building_out_of_range_number_id.clone())
        .entity_type(building_type)
        .build()?;
    // The model refuses it (a number outside the range of a double: 1e-16384), so `reading` is written as text.
    let reading: RawJson = r#"{"type": "Property", "value": 1e-16384}"#.parse()?;
    let document = UncheckedDocument::from_entity(&building_out_of_range_number, Lossless::Normalized).set_member("reading", &reading);
    let request = broker
        .unchecked(&CreateEntity::builder().entity(&building_out_of_range_number).build())?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_out_of_range_number_id)
        .send()
        .await;
    expect_that!(&answer_2, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    Ok(())
}
