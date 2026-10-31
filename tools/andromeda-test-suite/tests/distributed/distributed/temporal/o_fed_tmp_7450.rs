//! A dataset a per attribute source hid is probed on its own.

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
/// location was observed at 10:00 inside the fence when a source applying `lastN` per Attribute answers the probe
/// with dataset d2 alone and, asked again for d1 by `datasetId`, with d1 observed at 11:00 outside; the follow-up
/// probes name d1
/// Clause: 5.7.4.4, 4.11, 4.5.5, 6.18.3.2
#[gtest]
#[tokio::test]
async fn o_fed_tmp_7450_a_dataset_a_per_attribute_source_hid_is_probed_on_its_own() -> Result<()> {
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
    let source_evolutions = [TemporalEntity::builder()
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
                .query("datasetId", format!("urn:ngsi-ld:dataset:{namespace}-d1"))
                .priority(1),
            source_answer::temporal_entities(&source_evolutions),
        )
        .await?;
    let source_evolutions_2 = [TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .geo_property(
            "location",
            GeoProperty::builder(Shape::Point(Position::try_new(30.0, 30.0)?))
                .dataset_id(namespace.dataset_id("dataset", "d2")?)
                .observed_at("2020-08-01T11:40:00Z".parse()?),
        )
        .build()?];
    source
        .answer_when(
            SourceRequest::new(Method::GET, "/a/ngsi-ld/v1/temporal/entities")
                .query("timerel", "before")
                .query("timeproperty", "observedAt")
                .priority(2),
            source_answer::temporal_entities(&source_evolutions_2),
        )
        .await?;
    let source_evolutions_3 = [];
    source
        .answer_when(
            SourceRequest::new(Method::GET, "/a/ngsi-ld/v1/temporal/entities")
                .query("timerel", "before")
                .query("timeproperty", "deletedAt")
                .priority(3),
            source_answer::temporal_entities(&source_evolutions_3),
        )
        .await?;
    let source_evolutions_4 = [];
    source
        .answer_when(
            SourceRequest::new(Method::GET, "/a/ngsi-ld/v1/temporal/entities").priority(10),
            source_answer::temporal_entities(&source_evolutions_4),
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
    let forwarded_request =
        SourceRequest::new(Method::GET, "/a/ngsi-ld/v1/temporal/entities").query("datasetId", format!("urn:ngsi-ld:dataset:{namespace}-d1"));
    let forwarded = source.received_matching(&forwarded_request).await?;
    expect_that!(forwarded, len(eq(2)));
    Ok(())
}
