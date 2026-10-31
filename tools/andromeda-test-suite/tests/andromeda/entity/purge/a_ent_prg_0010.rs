//! A dataset scoped drop deletes only the named instance.

use andromeda_test_suite::{
    attribute_matcher::property,
    attribute_member::Member,
    broker,
    entity_matcher::has_attribute,
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

/// Pins: Purge Entities with drop and a datasetId answers 204 and deletes only the instance of the dropped attribute
/// with that datasetId, leaving its default instance and the other attributes
/// Clause: 5.6.21.4, 6.4.3.3
/// Recorded: Andromeda extension, not v1.9.1: clause 5.6.21.3 gives Purge Entities no datasetId and no deleteAll;
/// the `purge_dataset_scope` extension adds them
#[gtest]
#[tokio::test]
async fn a_ent_prg_0010_a_dataset_scoped_drop_deletes_only_the_named_instance() -> Result<()> {
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
    // Purge Entities defines no datasetId, so the parameter is written as the test needs it.
    let request = request_to(
        &broker,
        Method::DELETE,
        &format!("entities?type={vehicle_type}&id=urn%3Angsi-ld%3AVehicle%3A{namespace}-1&drop=speed&datasetId=urn%3Angsi-ld%3ADataset%3A{namespace}-one"),
    )?;
    expect_that!(&broker.send_unchecked(request).await, answered(StatusCode::NO_CONTENT));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("speed", property().value(10).without(Member::DatasetId)));
    expect_that!(entity, has_attribute("brandName", property().value("Mercedes")));
    Ok(())
}
