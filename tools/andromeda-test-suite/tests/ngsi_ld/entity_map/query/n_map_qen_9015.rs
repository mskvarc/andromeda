//! An entity map of a query with a datasetId lists only the registrations sharing it or listing none.

use andromeda_test_suite::{
    broker,
    dataset_sources::{DatasetBinding, DatasetBoundSources},
    header_matcher::present,
    namespace::Namespace,
    received_status::ExpectedStatus,
    source_answer,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::{headers::Accept, operation::EntityMapRequest},
    model::{Entity, NgsiLdMediaType, Output, Property},
    registration::{DistributedOperation, EntityInfo, EntitySource, RegistrationInfo},
};

/// Pins: over inclusive registrations of `Vehicle` Entities listing the `datasetId`s [A] (source a) and [B] (source
/// b), and one listing none (source n), each source holding one Entity of its own, Query Entities by type with
/// `datasetId` A and `entityMap` true answers 201, and the `EntityMap` it names maps the Entity of source a to its
/// registration and the Entity of source n to its registration, and holds no Entity of source b
/// Clause: 5.12, 5.7.2.4, 5.2.39, 4.3.6.7
#[gtest]
#[tokio::test]
async fn n_map_qen_9015_an_entity_map_of_a_query_with_a_dataset_id_lists_only_the_registrations_sharing_it_or_listing_none() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let dataset_a = namespace.dataset_id("Dataset", "a")?;
    let sources = DatasetBoundSources::register(
        &broker,
        &namespace,
        &RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?,
        &[DistributedOperation::QueryEntity],
        vec![
            DatasetBinding {
                prefix: "a",
                dataset_ids: Some(vec![dataset_a.clone()]),
            },
            DatasetBinding {
                prefix: "b",
                dataset_ids: Some(vec![namespace.dataset_id("Dataset", "b")?]),
            },
            DatasetBinding {
                prefix: "n",
                dataset_ids: None,
            },
        ],
    )
    .await?;
    let mut held: Vec<(&str, Entity<Output>)> = Vec::new();
    for prefix in ["a", "b", "n"] {
        held.push((
            prefix,
            Entity::output_builder()
                .id(namespace.urn("Vehicle", prefix)?)
                .entity_type(vehicle_type.clone())
                .property("speed", Property::builder(50).dataset_id(dataset_a.clone()))
                .build()?,
        ));
    }
    let queried =
        |prefix: &str| SourceRequest::new(Method::GET, &format!("/{prefix}/ngsi-ld/v1/entities")).query_containing("type", format!("Vehicle_{namespace}"));
    sources
        .answer(queried, |prefix| {
            let own: Vec<Entity<Output>> = held.iter().filter(|(holder, _)| *holder == prefix).map(|(_, entity)| entity.clone()).collect();
            source_answer::entities(&own)
        })
        .await?;
    let page = broker
        .with_heads()
        .with_accept(Accept::any())
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .dataset_ids(vec![dataset_a.clone()])
        .entity_map(&EntityMapRequest::returned())
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    expect_that!(page.entity_map(), present());
    let map_id = page.entity_map().valid().or_fail()?.id().or_fail()?;
    let map = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_maps()
        .retrieve(&map_id)
        .send()
        .await?;
    expect_that!(
        map.entity_map.get(&namespace.urn("Vehicle", "a")?),
        some(contains(eq(&EntitySource::Registration(namespace.registration_id("a")?))))
    );
    expect_that!(
        map.entity_map.get(&namespace.urn("Vehicle", "n")?),
        some(contains(eq(&EntitySource::Registration(namespace.registration_id("n")?))))
    );
    expect_that!(map.entity_map.get(&namespace.urn("Vehicle", "b")?), none());
    let reached = sources.reached(|prefix| queried(prefix).query("datasetId", &dataset_a)).await?;
    expect_that!(reached, eq(&vec!["a", "n"]));
    Ok(())
}
