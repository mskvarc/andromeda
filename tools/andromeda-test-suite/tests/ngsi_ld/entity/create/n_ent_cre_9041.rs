//! An observed at whose year is not four digits is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::CreateEntity},
    model::{Entity, ErrorType, Lossless, NgsiLdMediaType, RawJson},
};

/// Pins: Create Entity with `observedAt` -4714-01-01T00:00:00Z, and with `observedAt` 300000-01-01T00:00:00Z, each
/// answer 400 `BadRequestData`, and neither Entity exists
/// Clause: 4.6.3, 5.6.1, 5.5.4
#[gtest]
#[tokio::test]
async fn n_ent_cre_9041_an_observed_at_whose_year_is_not_four_digits_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_year_below_id = namespace.urn("Vehicle", "year-below")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_year_above_id = namespace.urn("Vehicle", "year-above")?;
    let vehicle_year_below = Entity::builder().id(vehicle_year_below_id.clone()).entity_type(vehicle_type.clone()).build()?;
    // The model refuses it (invalid timestamp at inspected), so `inspected` is written as text.
    let inspected: RawJson =
        r#"{"type": "Property", "value": {"@type": "DateTime", "@value": "2026-03-04T05:06:07.123456Z"}, "observedAt": "-4714-01-01T00:00:00Z"}"#.parse()?;
    let document = UncheckedDocument::from_entity(&vehicle_year_below, Lossless::Normalized).set_member("inspected", &inspected);
    let request = broker
        .unchecked(&CreateEntity::builder().entity(&vehicle_year_below).build())?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_year_below_id)
        .send()
        .await;
    expect_that!(&answer_2, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    let vehicle_year_above = Entity::builder().id(vehicle_year_above_id.clone()).entity_type(vehicle_type.clone()).build()?;
    // The model refuses it (invalid timestamp at inspected), so `inspected` is written as text.
    let inspected_2: RawJson =
        r#"{"type": "Property", "value": {"@type": "DateTime", "@value": "2026-03-04T05:06:07.123456Z"}, "observedAt": "300000-01-01T00:00:00Z"}"#.parse()?;
    let document_2 = UncheckedDocument::from_entity(&vehicle_year_above, Lossless::Normalized).set_member("inspected", &inspected_2);
    let request_2 = broker
        .unchecked(&CreateEntity::builder().entity(&vehicle_year_above).build())?
        .document(&document_2);
    let answer_3 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_4 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_year_above_id)
        .send()
        .await;
    expect_that!(&answer_4, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    Ok(())
}
