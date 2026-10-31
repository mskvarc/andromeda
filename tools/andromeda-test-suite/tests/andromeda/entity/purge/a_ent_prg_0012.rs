//! A dataset scoped keep spares the kept attribute whole.

use andromeda_test_suite::{
    attribute_matcher::property,
    attribute_member::Member,
    broker,
    entity_matcher::{has_attribute, has_instances},
    namespace::Namespace,
    outcome::Done,
    problem::answered,
    wire::request_to,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{DatasetId, Entity, NgsiLdMediaType, Property},
};

/// Pins: Purge Entities with keep and a datasetId answers 204 and deletes the instance with that datasetId of every
/// attribute but the kept one, leaving the default instances and the kept attribute whole
/// Clause: 5.6.21.4, 6.4.3.3
/// Recorded: Andromeda extension, not v1.9.1: clause 5.6.21.3 gives Purge Entities no datasetId and no deleteAll;
/// the `purge_dataset_scope` extension adds them
#[gtest]
#[tokio::test]
async fn a_ent_prg_0012_a_dataset_scoped_keep_spares_the_kept_attribute_whole() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let dataset_one = namespace.dataset_id("Dataset", "one")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(10))
        .property("speed", Property::builder(20).dataset_id(dataset_one.clone()))
        .property("brandName", Property::builder("Mercedes"))
        .property("brandName", Property::builder("Benz").dataset_id(dataset_one.clone()))
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    // Purge Entities defines no datasetId, so the parameter is written as the test needs it.
    let request = request_to(
        &broker,
        Method::DELETE,
        &format!("entities?type={vehicle_type}&id=urn%3Angsi-ld%3AVehicle%3A{namespace}-1&keep=speed&datasetId=urn%3Angsi-ld%3ADataset%3A{namespace}-one"),
    )?;
    expect_that!(&broker.send_unchecked(request).await, answered(StatusCode::NO_CONTENT));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_instances("speed", 2));
    expect_that!(entity, has_attribute("speed", property().dataset_id(dataset_one).value(20)));
    expect_that!(entity, has_attribute("speed", property().dataset_id(DatasetId::DefaultInstance).value(10)));
    expect_that!(entity, has_attribute("brandName", property().value("Mercedes").without(Member::DatasetId)));
    Ok(())
}
