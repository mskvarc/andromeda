//! A purge is narrowed by the attributes it keeps.

use andromeda_test_suite::{
    attribute_matcher::property,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
    problem::{answered, problem},
    wire::request_to,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, NgsiLdMediaType, Property},
};

/// Pins: Purge Entities naming an id and keep answers 204, and naming only the id answers 400 `BadRequestData` (too
/// wide)
/// Clause: 5.6.21.3, 5.6.21.4
#[gtest]
#[tokio::test]
async fn n_ent_prg_9007_a_purge_is_narrowed_by_the_attributes_it_keeps() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_purge_narrowed_id = namespace.urn("Vehicle", "purge-narrowed")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let speed = namespace.attribute("speed")?;
    let vehicle_purge_narrowed = Entity::builder()
        .id(vehicle_purge_narrowed_id.clone())
        .entity_type(vehicle_type)
        .property(
            namespace.attribute("speed")?,
            Property::builder(60).observed_at("2025-01-01T00:00:00Z".parse()?),
        )
        .build()?;
    broker.entities().create(&vehicle_purge_narrowed).send().await?.done()?;
    let answer = broker
        .with_responses()
        .entities()
        .purge()
        .ids(vec![vehicle_purge_narrowed_id.clone()])
        .keep(vec![speed.clone()])
        .send()
        .await;
    expect_that!(&answer, answered(StatusCode::NO_CONTENT));
    // The client refuses an id-only purge before sending, so the request is sent as written.
    let request_2 = request_to(
        &broker,
        Method::DELETE,
        &format!("entities?id=urn%3Angsi-ld%3AVehicle%3A{namespace}-purge-narrowed"),
    )?;
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_purge_narrowed_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute(speed.as_ref(), property().value(60)));
    broker.entities().delete(&vehicle_purge_narrowed_id).send().await?.done()?;
    Ok(())
}
