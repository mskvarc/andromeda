//! An observed at with nine fraction digits is a bad request data error.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    attribute_member::Member,
    broker,
    entity_matcher::{has_attribute, lacks_attribute},
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    partial_outcome::reported,
    report_matcher::batch_error,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{BatchOperationResult, UncheckedDocument, headers::Accept, operation::BatchUpdate},
    model::{Entity, EntityFragment, ErrorType, Lossless, NgsiLdMediaType, Property, RawJson},
};

/// Pins: Batch Entity Update answers 207 with a `BadRequestData` error for the Entity whose `observedAt` has nine
/// fractional-second digits, which stays unchanged, and updates the other Entity
/// Clause: 4.6.3, 5.6.9.4, 5.5.4
#[gtest]
#[tokio::test]
async fn n_bat_upd_2703_an_observed_at_with_nine_fraction_digits_is_a_bad_request_data_error() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_2_id = namespace.urn("Vehicle", 2)?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(1).observed_at("2026-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let vehicle_2 = Entity::builder()
        .id(vehicle_2_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(1).observed_at("2026-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.entities().create(&vehicle_2).send().await?.done()?;
    let vehicle_fragment = EntityFragment::for_entity(vehicle_id.clone(), vehicle_type.clone()).build()?;
    // The model refuses it (invalid timestamp at speed), so `speed` is written as text.
    let speed: RawJson = r#"{"type": "Property", "value": 2, "observedAt": "2026-03-04T05:06:07.123456789Z"}"#.parse()?;
    let document = UncheckedDocument::try_from(vehicle_fragment.render(Lossless::Normalized))?.set_member("speed", &speed);
    let vehicle_2_fragment = EntityFragment::for_entity(vehicle_2_id.clone(), vehicle_type.clone())
        .property("speed", Property::builder(2).observed_at("2026-03-04T05:06:07.123456Z".parse()?))
        .build()?;
    let document_2 = UncheckedDocument::try_from(vehicle_2_fragment.render(Lossless::Normalized))?;
    let fragments = [vehicle_fragment, vehicle_2_fragment];
    let request = broker
        .unchecked(&BatchUpdate::builder().fragments(&fragments).build())?
        .documents(&[document, document_2]);
    let answer = broker.send_unchecked(request).await;
    let report: BatchOperationResult = reported(answer)?;
    expect_that!(&report.success, len(eq(1)));
    expect_that!(&report.success, nth(0, eq(&vehicle_2_id)));
    expect_that!(&report.errors, len(eq(1)));
    expect_that!(&report.errors, nth(0, batch_error().entity(&vehicle_id).problem(ErrorType::BadRequestData)));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_2_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(
        entity,
        has_attribute("speed", any_attribute().value(2).observed_at("2026-03-04T05:06:07.123456Z".parse()?))
    );
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(entity_2, lacks_attribute("heading"));
    expect_that!(
        entity_2,
        has_attribute(
            "speed",
            any_attribute().value(1).observed_at("2026-01-01T00:00:00Z".parse()?).without(Member::ExpiresAt)
        )
    );
    Ok(())
}
