//! A dataset id filters the aggregated history.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    attribute_member::Member,
    broker,
    config::{BrokerRole, PeerRole},
    evolution_matcher::{has_evolution, lacks_evolution},
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{DatasetId, Property, evolution::TemporalEntity},
    query::temporal_query,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo},
};

/// Pins: Query Temporal Evolution of Entities answers both speed instances without a `datasetId`, only the dataset
/// instance with `datasetId`, only the default one with @none, and no speed with a `datasetId` no instance carries
/// Clause: 5.7.4.4, 4.5.5.1
#[gtest]
#[tokio::test]
async fn n_fed_tmp_7410_a_dataset_id_filters_the_aggregated_history() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let broker_b = broker::client(BrokerRole::B)?;
    let dataset_2 = namespace.dataset_id("Dataset", 2)?;
    let dataset_absent = namespace.dataset_id("Dataset", "absent")?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(10).observed_at("2020-08-01T12:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
    let vehicle_evolution_2 = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property(
            "speed",
            Property::builder(30).dataset_id(dataset_2.clone()).observed_at("2020-08-01T12:05:00Z".parse()?),
        )
        .build()?;
    broker_b.temporal().create_or_update(&vehicle_evolution_2).send().await?.created()?;
    let registration = CSourceRegistration::builder(broker::peer(PeerRole::B)?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .operations([DistributedOperation::QueryTemporal])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    let page = broker
        .with_accept(Accept::any())
        .temporal()
        .query(temporal_query::before("2030-01-01T00:00:00Z".parse()?))
        .entity_type(&vehicle_type)
        .send()
        .await?;
    expect_that!(
        page.items(),
        nth(
            0,
            has_evolution(
                "speed",
                all!(
                    len(eq(2)),
                    contains(any_attribute().value(10)),
                    contains(any_attribute().with_dataset_id(dataset_2.clone()))
                )
            )
        )
    );
    let page_2 = broker
        .with_accept(Accept::any())
        .temporal()
        .query(temporal_query::before("2030-01-01T00:00:00Z".parse()?))
        .entity_type(&vehicle_type)
        .dataset_ids(vec![dataset_2.clone()])
        .send()
        .await?;
    expect_that!(
        page_2.items(),
        nth(
            0,
            has_evolution("speed", all!(len(eq(1)), nth(0, any_attribute().with_dataset_id(dataset_2.clone()))))
        )
    );
    let page_3 = broker
        .with_accept(Accept::any())
        .temporal()
        .query(temporal_query::before("2030-01-01T00:00:00Z".parse()?))
        .entity_type(&vehicle_type)
        .dataset_ids(vec![DatasetId::DefaultInstance])
        .send()
        .await?;
    expect_that!(
        page_3.items(),
        nth(
            0,
            has_evolution("speed", all!(len(eq(1)), nth(0, any_attribute().value(10).without(Member::DatasetId))))
        )
    );
    let page_4 = broker
        .with_accept(Accept::any())
        .temporal()
        .query(temporal_query::before("2030-01-01T00:00:00Z".parse()?))
        .entity_type(&vehicle_type)
        .dataset_ids(vec![dataset_absent])
        .send()
        .await?;
    expect_that!(page_4.items(), each(lacks_evolution("speed")));
    Ok(())
}
