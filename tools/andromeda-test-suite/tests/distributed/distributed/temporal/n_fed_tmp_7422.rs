//! A filter is applied to the evolution assembled from its parts.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    config::{BrokerRole, PeerRole},
    evolution_matcher::has_evolution,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        GeoProperty,
        Property,
        evolution::TemporalEntity,
        geometry::{Position, Shape},
    },
    query::{EntitySplitting, q, temporal_query},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo},
};

/// Pins: Query Temporal Evolution of Entities with `splitEntities` true and q speed>50 answers the Entity A, B and
/// C together hold with speed 60 and not the one whose merged speed is 40, and without a filter answers both merged
/// Clause: 5.7.4.4
#[gtest]
#[tokio::test]
async fn n_fed_tmp_7422_a_filter_is_applied_to_the_evolution_assembled_from_its_parts() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let broker_b = broker::client(BrokerRole::B)?;
    let broker_c = broker::client(BrokerRole::C)?;
    let vehicle_2_id = namespace.urn("Vehicle", 2)?;
    let registration_id = namespace.registration_id(1)?;
    let registration_2_id = namespace.registration_id(2)?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("fuelLevel", Property::builder(70).observed_at("2020-08-01T12:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
    let vehicle_evolution_2 = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(60).observed_at("2020-08-01T12:00:00Z".parse()?))
        .build()?;
    broker_b.temporal().create_or_update(&vehicle_evolution_2).send().await?.created()?;
    let vehicle_evolution_3 = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .geo_property(
            "location",
            GeoProperty::builder(Shape::Point(Position::try_new(8.0, 44.0)?)).observed_at("2020-08-01T12:00:00Z".parse()?),
        )
        .build()?;
    broker_c.temporal().create_or_update(&vehicle_evolution_3).send().await?.created()?;
    let vehicle_2_evolution = TemporalEntity::builder()
        .id(vehicle_2_id.clone())
        .entity_type(vehicle_type.clone())
        .property("fuelLevel", Property::builder(70).observed_at("2020-08-01T12:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle_2_evolution).send().await?.created()?;
    let vehicle_2_evolution_2 = TemporalEntity::builder()
        .id(vehicle_2_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(40).observed_at("2020-08-01T12:00:00Z".parse()?))
        .build()?;
    broker_b.temporal().create_or_update(&vehicle_2_evolution_2).send().await?.created()?;
    let vehicle_2_evolution_3 = TemporalEntity::builder()
        .id(vehicle_2_id.clone())
        .entity_type(vehicle_type.clone())
        .geo_property(
            "location",
            GeoProperty::builder(Shape::Point(Position::try_new(8.0, 44.0)?)).observed_at("2020-08-01T12:00:00Z".parse()?),
        )
        .build()?;
    broker_c.temporal().create_or_update(&vehicle_2_evolution_3).send().await?.created()?;
    let registration = CSourceRegistration::builder(broker::peer(PeerRole::B)?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .operations([DistributedOperation::QueryTemporal])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    let registration_2 = CSourceRegistration::builder(broker::peer(PeerRole::C)?)
        .id(registration_2_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .operations([DistributedOperation::QueryTemporal])
        .build()?;
    broker.registrations().register(&registration_2).send().await?;
    let page = broker
        .with_accept(Accept::any())
        .temporal()
        .query(temporal_query::before("2030-01-01T00:00:00Z".parse()?))
        .entity_type(&vehicle_type)
        .split_entities(EntitySplitting::Split)
        .q(q::attr("speed").gt(50))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(
        page.items(),
        nth(
            0,
            all!(
                has_id(&vehicle_id),
                has_evolution("fuelLevel", len(gt(0))),
                has_evolution("speed", nth(0, any_attribute().value(60))),
                has_evolution("location", len(gt(0)))
            )
        )
    );
    let page_2 = broker
        .with_accept(Accept::any())
        .temporal()
        .query(temporal_query::before("2030-01-01T00:00:00Z".parse()?))
        .entity_type(&vehicle_type)
        .split_entities(EntitySplitting::Split)
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(2)));
    expect_that!(
        page_2.items(),
        contains(all!(
            has_id(&vehicle_id),
            has_evolution("speed", len(gt(0))),
            has_evolution("location", len(gt(0))),
            has_evolution("fuelLevel", len(gt(0)))
        ))
    );
    expect_that!(
        page_2.items(),
        contains(all!(
            has_id(&vehicle_2_id),
            has_evolution("speed", len(gt(0))),
            has_evolution("location", len(gt(0))),
            has_evolution("fuelLevel", len(gt(0)))
        ))
    );
    Ok(())
}
