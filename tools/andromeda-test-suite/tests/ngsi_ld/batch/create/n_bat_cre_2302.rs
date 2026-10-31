//! An observed at with a comma fraction is accepted.

use andromeda_test_suite::{attribute_matcher::any_attribute, broker, entity_matcher::has_attribute, namespace::Namespace, problem::answered};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::BatchCreate},
    model::{Entity, Lossless, NgsiLdMediaType, RawJson},
};

/// Pins: Batch Entity Creation accepts an `observedAt` whose seconds fraction is separated by a comma
/// Clause: 4.6.3, 5.6.7
#[gtest]
#[tokio::test]
async fn n_bat_cre_2302_an_observed_at_with_a_comma_fraction_is_accepted() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle = Entity::builder().id(vehicle_id.clone()).entity_type(vehicle_type).build()?;
    // The model refuses it (the model reads it as written otherwise [at $.speed.observedAt (written "2024-01-01T00:00:00,5Z")]: {"id":"urn:ngsi-ld:Vehicle:zqns-1","type":"Vehicle_zqns","speed":{"type":"Property","value":1,"observedAt":"2024-01-01T00:00:00.5Z"}}), so `speed` is written as text.
    let speed: RawJson = r#"{"type": "Property", "value": 1, "observedAt": "2024-01-01T00:00:00,5Z"}"#.parse()?;
    let document = UncheckedDocument::from_entity(&vehicle, Lossless::Normalized).set_member("speed", &speed);
    let entities = [vehicle];
    let request = broker.unchecked(&BatchCreate::builder().entities(&entities).build())?.documents(&[document]);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("speed", any_attribute().observed_at("2024-01-01T00:00:00.5Z".parse()?)));
    Ok(())
}
