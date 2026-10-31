//! An observed at with nine fraction digits is bad request data.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    attribute_member::Member,
    broker,
    entity_matcher::{has_attribute, lacks_attribute},
    namespace::Namespace,
    outcome::Done,
    problem::problem,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::AppendAttributes},
    model::{Entity, EntityFragment, ErrorType, Lossless, NgsiLdMediaType, Property, RawJson},
};

/// Pins: Append Entity Attributes whose Fragment carries an `observedAt` of nine fractional-second digits answers
/// 400 `BadRequestData` and appends nothing
/// Clause: 4.6.3, 5.6.3.4, 5.5.4
#[gtest]
#[tokio::test]
async fn n_att_app_7602_an_observed_at_with_nine_fraction_digits_is_bad_request_data() -> Result<()> {
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
    let fragment = EntityFragment::builder().build()?;
    // The model refuses it (invalid timestamp at heading), so `heading` is written as text.
    let heading: RawJson = r#"{"type": "Property", "value": 90, "observedAt": "2026-03-04T05:06:07.123456789Z"}"#.parse()?;
    let document = UncheckedDocument::try_from(fragment.render(Lossless::Normalized))?.set_member("heading", &heading);
    let request = broker
        .unchecked(&AppendAttributes::builder().id(&vehicle_id).fragment(&fragment).build())?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
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
