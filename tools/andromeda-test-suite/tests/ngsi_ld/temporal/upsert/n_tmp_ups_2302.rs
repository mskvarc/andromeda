//! An observed at with a comma fraction is accepted.

use andromeda_test_suite::{attribute_matcher::any_attribute, broker, evolution_matcher::has_evolution, namespace::Namespace, nth::nth, problem::answered};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::CreateOrUpdateTemporalEvolution},
    model::{Lossless, NgsiLdMediaType, RawJson, evolution::TemporalEntity},
};

/// Pins: Create Temporal Evolution accepts instances whose `observedAt` has a comma before the seconds fraction, in
/// the normalized form and as the second element of a simplified pair
/// Clause: 4.6.3, 5.6.11
#[gtest]
#[tokio::test]
async fn n_tmp_ups_2302_an_observed_at_with_a_comma_fraction_is_accepted() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_2_id = namespace.urn("Vehicle", 2)?;
    let vehicle_evolution = TemporalEntity::builder().id(vehicle_id.clone()).entity_type(vehicle_type.clone()).build()?;
    // The model refuses it (the model reads it as written otherwise [at $.speed[0].observedAt (written "2020-08-01T12:00:00,5Z")]: {"id":"urn:ngsi-ld:Vehicle:zqns-1","type":"Vehicle_zqns","speed":[{"type":"Property","value":55,"observedAt":"2020-08-01T12:00:00.5Z"}]}), so `speed` is written as text.
    let speed: RawJson = r#"[{"type": "Property", "value": 55, "observedAt": "2020-08-01T12:00:00,5Z"}]"#.parse()?;
    let document = UncheckedDocument::try_from(vehicle_evolution.render(Lossless::Normalized))?.set_member("speed", &speed);
    let request = broker
        .unchecked(&CreateOrUpdateTemporalEvolution::builder().entity(&vehicle_evolution).build())?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let vehicle_2_evolution = TemporalEntity::builder().id(vehicle_2_id.clone()).entity_type(vehicle_type.clone()).build()?;
    // The model refuses it (expected a JSON object at speed.values), so `speed` is written as text.
    let speed_2: RawJson = r#"{"type": "Property", "values": [[55, "2020-08-01T12:00:00,5Z"]]}"#.parse()?;
    let document_2 = UncheckedDocument::try_from(vehicle_2_evolution.render(Lossless::Normalized))?.set_member("speed", &speed_2);
    let request_2 = broker
        .unchecked(&CreateOrUpdateTemporalEvolution::builder().entity(&vehicle_2_evolution).build())?
        .document(&document_2);
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, answered(StatusCode::CREATED));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(
        evolution,
        has_evolution(
            "speed",
            all!(len(eq(1)), nth(0, any_attribute().observed_at("2020-08-01T12:00:00.5Z".parse()?)))
        )
    );
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_2_id)
        .send()
        .await?;
    let evolution_2 = retrieved_2.body();
    expect_that!(
        evolution_2,
        has_evolution(
            "speed",
            all!(len(eq(1)), nth(0, any_attribute().observed_at("2020-08-01T12:00:00.5Z".parse()?)))
        )
    );
    Ok(())
}
