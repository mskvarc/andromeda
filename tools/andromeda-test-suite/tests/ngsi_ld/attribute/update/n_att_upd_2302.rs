//! An observed at with a comma fraction is accepted.

use andromeda_test_suite::{attribute_matcher::any_attribute, broker, entity_matcher::has_attribute, namespace::Namespace, outcome::Done, problem::answered};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::UpdateAttributes},
    model::{Entity, EntityFragment, Lossless, NgsiLdMediaType, RawJson},
};

/// Pins: Update Entity Attributes accepts an `observedAt` whose seconds fraction is separated by a comma
/// Clause: 4.6.3, 5.6.2
#[gtest]
#[tokio::test]
async fn n_att_upd_2302_an_observed_at_with_a_comma_fraction_is_accepted() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("speed", 1)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let fragment = EntityFragment::builder().build()?;
    // The model refuses it (the model reads it as written otherwise [at $.speed.observedAt (written "2024-01-01T00:00:00,5Z")]: {"speed":{"type":"Property","value":2,"observedAt":"2024-01-01T00:00:00.5Z"}}), so `speed` is written as text.
    let speed: RawJson = r#"{"type": "Property", "value": 2, "observedAt": "2024-01-01T00:00:00,5Z"}"#.parse()?;
    let document = UncheckedDocument::try_from(fragment.render(Lossless::Normalized))?.set_member("speed", &speed);
    let request = broker
        .unchecked(&UpdateAttributes::builder().id(&vehicle_id).fragment(&fragment).build())?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::NO_CONTENT));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(
        entity,
        has_attribute("speed", any_attribute().value(2).observed_at("2024-01-01T00:00:00.5Z".parse()?))
    );
    Ok(())
}
