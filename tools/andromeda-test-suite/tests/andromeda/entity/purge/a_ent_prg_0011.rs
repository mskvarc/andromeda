//! Delete all deletes every instance of the dropped attribute.

use andromeda_test_suite::{
    attribute_matcher::property,
    broker,
    entity_matcher::{has_attribute, lacks_attribute},
    namespace::Namespace,
    outcome::Done,
    problem::answered,
    wire::request_to,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, Property},
};

/// Pins: Purge Entities with drop and deleteAll=true answers 204 and deletes every instance of the dropped
/// attribute, leaving the other attributes
/// Clause: 5.6.21.4, 6.4.3.3
/// Recorded: Andromeda extension, not v1.9.1: clause 5.6.21.3 gives Purge Entities no datasetId and no deleteAll;
/// the `purge_dataset_scope` extension adds them
#[gtest]
#[tokio::test]
async fn a_ent_prg_0011_delete_all_deletes_every_instance_of_the_dropped_attribute() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(10))
        .property("speed", Property::builder(20).dataset_id(namespace.dataset_id("Dataset", "one")?))
        .property("brandName", "Mercedes")
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    // Purge Entities defines no deleteAll, so the parameter is written as the test needs it.
    let request = request_to(
        &broker,
        Method::DELETE,
        &format!("entities?type={vehicle_type}&id=urn%3Angsi-ld%3AVehicle%3A{namespace}-1&drop=speed&deleteAll=true"),
    )?;
    expect_that!(&broker.send_unchecked(request).await, answered(StatusCode::NO_CONTENT));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity.id, eq(&vehicle_id));
    expect_that!(entity, lacks_attribute("speed"));
    expect_that!(entity, has_attribute("brandName", property().value("Mercedes")));
    Ok(())
}
