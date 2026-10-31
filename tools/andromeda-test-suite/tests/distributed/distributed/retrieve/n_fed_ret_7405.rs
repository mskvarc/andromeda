//! Conflicting default instances merge by observed at and datasets coexist.

use andromeda_test_suite::{
    attribute_matcher::{geo_property, property},
    attribute_member::Member,
    broker,
    config::{BrokerRole, PeerRole},
    entity_matcher::{has_attribute, has_instance, has_instances},
    namespace::Namespace,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        Entity,
        Property,
        geometry::{Position, Shape},
    },
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo},
};

/// Pins: Retrieve Entity on broker A merges the default speed of A (`observedAt` 2021) with that of broker B
/// (`observedAt` 2022) into the more recent one and keeps the `datasetId` speed and the location of broker C beside
/// it
/// Clause: 5.7.1.4, 4.5.5.3
#[gtest]
#[tokio::test]
async fn n_fed_ret_7405_conflicting_default_instances_merge_by_observed_at_and_datasets_coexist() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let broker_b = broker::client(BrokerRole::B)?;
    let broker_c = broker::client(BrokerRole::C)?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let dataset_2 = namespace.dataset_id("Dataset", 2)?;
    let registration_id = namespace.registration_id(1)?;
    let registration_2_id = namespace.registration_id(2)?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(10).observed_at("2021-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let vehicle_b = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(20).observed_at("2022-01-01T00:00:00Z".parse()?))
        .build()?;
    broker_b.entities().create(&vehicle_b).send().await?.done()?;
    let vehicle_c = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property(
            "speed",
            Property::builder(30).observed_at("2023-01-01T00:00:00Z".parse()?).dataset_id(dataset_2.clone()),
        )
        .geo_property("location", Shape::Point(Position::try_new(1.0, 2.0)?))
        .build()?;
    broker_c.entities().create(&vehicle_c).send().await?.done()?;
    let registration = CSourceRegistration::builder(broker::peer(PeerRole::B)?)
        .id(registration_id)
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_id.clone())])
            .property_names(["speed"])
            .build()?])
        .operations([DistributedOperation::RetrieveEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    let registration_2 = CSourceRegistration::builder(broker::peer(PeerRole::C)?)
        .id(registration_2_id)
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_id.clone())])
            .property_names(["speed", "location"])
            .build()?])
        .operations([DistributedOperation::RetrieveEntity])
        .build()?;
    broker.registrations().register(&registration_2).send().await?;
    let retrieved = broker.with_accept(Accept::any()).entities().retrieve(&vehicle_id).send().await?;
    let entity = retrieved.body();
    expect_that!(
        entity,
        has_attribute("location", geo_property().geometry(Shape::Point(Position::try_new(1.0, 2.0)?)))
    );
    expect_that!(entity, has_instances("speed", 2));
    expect_that!(entity, has_instance("speed", property().value(20).without(Member::DatasetId)));
    expect_that!(entity, not(has_instance("speed", property().value(10))));
    expect_that!(entity, has_instance("speed", property().with_dataset_id(dataset_2)));
    Ok(())
}
