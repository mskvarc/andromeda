//! A dataset id filters the merged history and removes emptied attributes.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    attribute_member::Member,
    broker,
    config::{BrokerRole, PeerRole},
    evolution_matcher::{has_evolution, lacks_evolution},
    namespace::Namespace,
    outcome::Upserted,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{DatasetId, Property, evolution::TemporalEntity},
    query::temporal_query,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo},
};

/// Pins: Retrieve Temporal Evolution with `datasetId` keeps only the speed instances of that dataset, which only
/// broker B holds, with @none only the default one, and removes the attribute when no instance carries the
/// `datasetId`
/// Clause: 5.7.3.4, 4.5.5.1
#[gtest]
#[tokio::test]
async fn n_fed_tmp_7404_a_dataset_id_filters_the_merged_history_and_removes_emptied_attributes() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let broker_b = broker::client(BrokerRole::B)?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let dataset_2 = namespace.dataset_id("Dataset", 2)?;
    let dataset_absent = namespace.dataset_id("Dataset", "absent")?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(10).observed_at("2020-08-01T12:00:00Z".parse()?))
        .property(
            "fuelLevel",
            Property::builder(40).observed_at("2020-08-01T12:00:00Z".parse()?).dataset_id(dataset_2.clone()),
        )
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
    let vehicle_evolution_2 = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property(
            "speed",
            Property::builder(30).observed_at("2020-08-01T12:05:00Z".parse()?).dataset_id(dataset_2.clone()),
        )
        .build()?;
    broker_b.temporal().create_or_update(&vehicle_evolution_2).send().await?.created()?;
    let registration = CSourceRegistration::builder(broker::peer(PeerRole::B)?)
        .id(registration_id)
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_id.clone())])
            .property_names(["speed"])
            .build()?])
        .operations([DistributedOperation::RetrieveTemporal])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    let retrieved = broker
        .with_accept(Accept::any())
        .temporal()
        .retrieve(&vehicle_id)
        .temporal_q(temporal_query::before("2030-01-01T00:00:00Z".parse()?))
        .dataset_ids(vec![dataset_2.clone()])
        .send()
        .await?;
    expect_that!(
        retrieved.body(),
        has_evolution(
            "speed",
            all!(
                contains(any_attribute().with_dataset_id(dataset_2.clone())),
                not(contains(any_attribute().value(10)))
            )
        )
    );
    let retrieved_2 = broker
        .with_accept(Accept::any())
        .temporal()
        .retrieve(&vehicle_id)
        .temporal_q(temporal_query::before("2030-01-01T00:00:00Z".parse()?))
        .dataset_ids(vec![DatasetId::DefaultInstance])
        .send()
        .await?;
    expect_that!(
        retrieved_2.body(),
        has_evolution(
            "speed",
            all!(
                contains(any_attribute().value(10)),
                not(contains(any_attribute().with_member(Member::DatasetId)))
            )
        )
    );
    let retrieved_3 = broker
        .with_accept(Accept::any())
        .temporal()
        .retrieve(&vehicle_id)
        .temporal_q(temporal_query::before("2030-01-01T00:00:00Z".parse()?))
        .dataset_ids(vec![dataset_absent])
        .send()
        .await?;
    expect_that!(retrieved_3.body(), lacks_evolution("speed"));
    expect_that!(retrieved_3.body(), lacks_evolution("fuelLevel"));
    Ok(())
}
