//! A date time value with a numeric offset is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::CreateEntity},
    model::{Entity, ErrorType, Lossless, NgsiLdMediaType, RawJson},
};

/// Pins: Create Entity whose Property holds a `DateTime` value carrying a numeric offset in place of Z (+01:00, or
/// +00:00 naming UTC) answers 400 `BadRequestData`, and neither the Entity nor its Temporal Evolution exists
/// Clause: 4.6.3, 5.6.1, 5.5.4
#[gtest]
#[tokio::test]
async fn n_ent_cre_9044_a_date_time_value_with_a_numeric_offset_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_nine_digit_value_id = namespace.urn("Vehicle", "nine-digit-value")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_nine_digit_value = Entity::builder()
        .id(vehicle_nine_digit_value_id.clone())
        .entity_type(vehicle_type.clone())
        .build()?;
    // The model refuses it (invalid value at inspected), so `inspected` is written as text.
    let inspected: RawJson =
        r#"{"type": "Property", "value": {"@type": "DateTime", "@value": "2026-03-04T05:06:07+01:00"}, "observedAt": "2026-03-04T05:06:07.123456Z"}"#
            .parse()?;
    let document = UncheckedDocument::from_entity(&vehicle_nine_digit_value, Lossless::Normalized).set_member("inspected", &inspected);
    let request = broker
        .unchecked(&CreateEntity::builder().entity(&vehicle_nine_digit_value).build())?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let vehicle_nine_digit_value_2 = Entity::builder()
        .id(vehicle_nine_digit_value_id.clone())
        .entity_type(vehicle_type.clone())
        .build()?;
    // The model refuses it (invalid value at inspected), so `inspected` is written as text.
    let inspected_2: RawJson =
        r#"{"type": "Property", "value": {"@type": "DateTime", "@value": "2026-03-04T05:06:07+00:00"}, "observedAt": "2026-03-04T05:06:07.123456Z"}"#
            .parse()?;
    let document_2 = UncheckedDocument::from_entity(&vehicle_nine_digit_value_2, Lossless::Normalized).set_member("inspected", &inspected_2);
    let request_2 = broker
        .unchecked(&CreateEntity::builder().entity(&vehicle_nine_digit_value_2).build())?
        .document(&document_2);
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_3 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_nine_digit_value_id)
        .send()
        .await;
    expect_that!(&answer_3, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    let answer_4 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_nine_digit_value_id)
        .send()
        .await;
    expect_that!(&answer_4, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    Ok(())
}
