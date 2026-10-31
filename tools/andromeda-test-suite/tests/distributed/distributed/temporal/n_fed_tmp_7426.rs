//! The value filter and the geoquery may hold in different parts.

use andromeda_test_suite::{broker, cleanup::Cleanup, context_source::ContextSource, namespace::Namespace, outcome::Upserted, source_answer};
use googletest::prelude::*;
use http::Method;
use ngsi_ld::{
    http::{headers::Accept, operation::RepresentationSpelling},
    model::{
        GeoProperty,
        Property,
        evolution::TemporalEntity,
        geometry::{Position, Shape},
    },
    query::{EntitySplitting, geoquery, q, temporal_query},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo},
};

/// Pins: Query Temporal Evolution of Entities with `splitEntities` true, q speed>50 and a near `geoQ` answers the
/// Entity whose source location is inside the geofence and not the one whose source location is outside, both
/// holding local speed 60
/// Clause: 5.7.4.4, 4.10
#[gtest]
#[tokio::test]
async fn n_fed_tmp_7426_the_value_filter_and_the_geoquery_may_hold_in_different_parts() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_2_id = namespace.urn("Vehicle", 2)?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(60).observed_at("2020-08-01T12:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
    let vehicle_2_evolution = TemporalEntity::builder()
        .id(vehicle_2_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(60).observed_at("2020-08-01T12:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle_2_evolution).send().await?.created()?;
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
    let source_evolutions = [
        TemporalEntity::builder()
            .id(vehicle_id.clone())
            .entity_type(vehicle_type.clone())
            .geo_property(
                "location",
                GeoProperty::builder(Shape::Point(Position::try_new(8.0, 44.0)?)).observed_at("2020-08-01T12:00:00Z".parse()?),
            )
            .build()?,
        TemporalEntity::builder()
            .id(vehicle_2_id.clone())
            .entity_type(vehicle_type.clone())
            .geo_property(
                "location",
                GeoProperty::builder(Shape::Point(Position::try_new(9.0, 45.0)?)).observed_at("2020-08-01T12:00:00Z".parse()?),
            )
            .build()?,
    ];
    source
        .answer(
            Method::GET,
            "/a/ngsi-ld/v1/temporal/entities",
            source_answer::temporal_entities(&source_evolutions),
        )
        .await?;
    let page = broker
        .with_accept(Accept::any())
        .temporal()
        .query_values(temporal_query::before("2030-01-01T00:00:00Z".parse()?))
        .geo_q(geoquery::near_max(1000, Shape::Point(Position::try_new(8.0, 44.0)?)))
        .entity_type(&vehicle_type)
        .split_entities(EntitySplitting::Split)
        .q(q::attr("speed").gt(50))
        .spelling(RepresentationSpelling::default().in_options())
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    let values = page.items().first().or_fail()?;
    expect_that!(values.id, eq(&vehicle_id));
    Ok(())
}
