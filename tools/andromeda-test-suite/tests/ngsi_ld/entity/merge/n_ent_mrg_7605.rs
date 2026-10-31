//! An observed at parameter with a numeric offset is bad request data.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    attribute_member::Member,
    broker,
    entity_matcher::{has_attribute, lacks_attribute},
    namespace::Namespace,
    outcome::Done,
    problem::problem,
    wire::api_uri,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::MergeEntity},
    model::{Entity, EntityFragment, ErrorType, NgsiLdMediaType, Property},
};

/// Pins: Merge Entity with an `observedAt` request parameter carrying a numeric offset in place of Z (+01:00, or
/// +00:00 naming UTC) answers 400 `BadRequestData` and leaves the Entity unchanged
/// Clause: 4.6.3, 5.6.17.4, 6.5.3.4, 5.5.4
#[gtest]
#[tokio::test]
async fn n_ent_mrg_7605_an_observed_at_parameter_with_a_numeric_offset_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("speed", Property::builder(1).observed_at("2026-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let fragment = EntityFragment::builder().property("speed", 2).build()?;
    // The operation defines no `observedAt` parameter; the request carries it as written.
    let request = broker
        .unchecked(&MergeEntity::builder().id(&vehicle_id).fragment(&fragment).build())?
        .uri(api_uri(
            &broker,
            &format!("entities/{vehicle_id}?observedAt=2026-03-04T05%3A06%3A07%2B01%3A00"),
        )?);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let fragment_2 = EntityFragment::builder().property("speed", 2).build()?;
    // The operation defines no `observedAt` parameter; the request carries it as written.
    let request_2 = broker
        .unchecked(&MergeEntity::builder().id(&vehicle_id).fragment(&fragment_2).build())?
        .uri(api_uri(
            &broker,
            &format!("entities/{vehicle_id}?observedAt=2026-03-04T05%3A06%3A07%2B00%3A00"),
        )?);
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, lacks_attribute("heading"));
    expect_that!(
        entity,
        has_attribute(
            "speed",
            any_attribute().value(1).observed_at("2026-01-01T00:00:00Z".parse()?).without(Member::ExpiresAt)
        )
    );
    Ok(())
}
