//! A geo query asks a per attribute source for nothing before the window.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    namespace::Namespace,
    outcome::Upserted,
    source_answer,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::Method;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        GeoProperty,
        Property,
        evolution::TemporalEntity,
        geometry::{Position, Shape},
    },
    query::{EntitySplitting, geoquery, temporal_query},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo},
};

/// Pins: under `splitEntities` a between 12:00-13:00 near `geoQ` does not answer a Vehicle whose local dataset d1
/// location was observed at 10:00 inside the fence, and a source applying `lastN` per Attribute is asked for the
/// window alone: neither a request before the window nor one naming a `datasetId` reaches it
/// Clause: 5.7.4.4, 4.11, 4.5.5, 6.18.3.2
#[gtest]
#[tokio::test]
async fn o_fed_tmp_7450_a_geo_query_asks_a_per_attribute_source_for_the_window_alone() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .geo_property(
            "location",
            GeoProperty::builder(Shape::Point(Position::try_new(8.0, 44.0)?))
                .dataset_id(namespace.dataset_id("dataset", "d1")?)
                .observed_at("2020-08-01T10:00:00Z".parse()?),
        )
        .property("speed", Property::builder(60).observed_at("2020-08-01T12:10:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .operations([DistributedOperation::QueryTemporal])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let source_before = [TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .geo_property(
            "location",
            GeoProperty::builder(Shape::Point(Position::try_new(20.0, 20.0)?))
                .dataset_id(namespace.dataset_id("dataset", "d1")?)
                .observed_at("2020-08-01T11:00:00Z".parse()?),
        )
        .build()?];
    source
        .answer_when(
            SourceRequest::new(Method::GET, "/a/ngsi-ld/v1/temporal/entities")
                .query("timerel", "before")
                .query("timeproperty", "observedAt")
                .priority(1),
            source_answer::temporal_entities(&source_before),
        )
        .await?;
    let source_window = [TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .geo_property(
            "location",
            GeoProperty::builder(Shape::Point(Position::try_new(30.0, 30.0)?))
                .dataset_id(namespace.dataset_id("dataset", "d2")?)
                .observed_at("2020-08-01T12:20:00Z".parse()?),
        )
        .build()?];
    source
        .answer_when(
            SourceRequest::new(Method::GET, "/a/ngsi-ld/v1/temporal/entities").priority(10),
            source_answer::temporal_entities(&source_window),
        )
        .await?;
    let page = broker
        .with_accept(Accept::any())
        .temporal()
        .query(temporal_query::between("2020-08-01T12:00:00Z".parse()?, "2020-08-01T13:00:00Z".parse()?))
        .geo_q(geoquery::near_max(1000, Shape::Point(Position::try_new(8.0, 44.0)?)))
        .entity_type(&vehicle_type)
        .split_entities(EntitySplitting::Split)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(0)));
    let before_the_window = SourceRequest::new(Method::GET, "/a/ngsi-ld/v1/temporal/entities").query("timerel", "before");
    expect_that!(source.received_matching(&before_the_window).await?, len(eq(0)));
    let by_dataset = SourceRequest::new(Method::GET, "/a/ngsi-ld/v1/temporal/entities").query("datasetId", format!("urn:ngsi-ld:dataset:{namespace}-d1"));
    expect_that!(source.received_matching(&by_dataset).await?, len(eq(0)));
    Ok(())
}
