//! An observed at with nine fraction digits is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::CreateEntity},
    model::{Entity, ErrorType, Lossless, NgsiLdMediaType, RawJson},
};

/// Pins: Create Entity whose Property carries an `observedAt` of nine fractional-second digits answers 400
/// `BadRequestData`, and neither the Entity nor its Temporal Evolution exists
/// Clause: 4.6.3, 5.6.1, 5.5.4
#[gtest]
#[tokio::test]
async fn n_ent_cre_9039_an_observed_at_with_nine_fraction_digits_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_nine_digit_observed_id = namespace.urn("Vehicle", "nine-digit-observed")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_nine_digit_observed = Entity::builder().id(vehicle_nine_digit_observed_id.clone()).entity_type(vehicle_type).build()?;
    // The model refuses it (invalid timestamp at inspected), so `inspected` is written as text.
    let inspected: RawJson =
        r#"{"type": "Property", "value": {"@type": "DateTime", "@value": "2026-03-04T05:06:07.123456Z"}, "observedAt": "2026-03-04T05:06:07.123456789Z"}"#
            .parse()?;
    let document = UncheckedDocument::from_entity(&vehicle_nine_digit_observed, Lossless::Normalized).set_member("inspected", &inspected);
    let request = broker
        .unchecked(&CreateEntity::builder().entity(&vehicle_nine_digit_observed).build())?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_nine_digit_observed_id)
        .send()
        .await;
    expect_that!(&answer_2, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    let answer_3 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_nine_digit_observed_id)
        .send()
        .await;
    expect_that!(&answer_3, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    Ok(())
}
