//! A location a source held inside the window matches, one it held before the window does not.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
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

/// Pins: under `splitEntities` a between 12:00-13:00 near `geoQ` answers the Entity holding local data in the
/// window whose location a source observed at 12:30 inside the fence, and not the Entity holding local data in the
/// window whose only location the source observed at 11:00 inside the fence, before the window
/// Clause: 5.7.4.4, 4.11, 4.10
#[gtest]
#[tokio::test]
async fn o_fed_tmp_7432_a_source_location_matches_only_inside_the_window() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_2_id = namespace.urn("Vehicle", 2)?;
    for id in [&vehicle_id, &vehicle_2_id] {
        let vehicle_evolution = TemporalEntity::builder()
            .id(id.clone())
            .entity_type(vehicle_type.clone())
            .property("speed", Property::builder(60).observed_at("2020-08-01T12:10:00Z".parse()?))
            .build()?;
        broker.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
    }
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
        .id(vehicle_2_id.clone())
        .entity_type(vehicle_type.clone())
        .geo_property(
            "location",
            GeoProperty::builder(Shape::Point(Position::try_new(8.0, 44.0)?)).observed_at("2020-08-01T11:00:00Z".parse()?),
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
            GeoProperty::builder(Shape::Point(Position::try_new(8.0, 44.0)?)).observed_at("2020-08-01T12:30:00Z".parse()?),
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
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&vehicle_id)));
    let before_the_window = SourceRequest::new(Method::GET, "/a/ngsi-ld/v1/temporal/entities").query("timerel", "before");
    expect_that!(source.received_matching(&before_the_window).await?, len(eq(0)));
    Ok(())
}
