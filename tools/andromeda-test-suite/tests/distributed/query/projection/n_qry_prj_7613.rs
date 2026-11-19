//! A split query decides a geoquery on the merged entity before attrs reduces it.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    cleanup::Cleanup,
    config::{BrokerRole, PeerRole},
    entity_matcher::{has_attribute, lacks_attribute},
    identity_matcher::has_id,
    namespace::Namespace,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        Entity,
        geometry::{Position, Shape},
    },
    query::{EntitySplitting, geoquery},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Query Entities by type with `splitEntities` true, attrs speed and a near geoquery around Madrid, over an
/// inclusive registration of peer broker B, answers the Entity whose local part holds only its location in Madrid and
/// whose speed 70 B holds, reduced to speed, and not the Entity whose local part is located in Berlin and whose speed
/// 60 B holds
/// Clause: 5.7.2.4, 5.7.2.5, 4.3.6, 4.10
#[gtest]
#[tokio::test]
async fn n_qry_prj_7613_a_split_query_decides_a_geoquery_on_the_merged_entity_before_attrs_reduces_it() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let broker_b = broker::client(BrokerRole::B)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_2_id = namespace.urn("Vehicle", 2)?;
    let registration_id = namespace.registration_id(1)?;
    let madrid = Shape::Point(Position::try_new(-3.7038, 40.4168)?);
    let berlin = Shape::Point(Position::try_new(13.405, 52.52)?);
    for (id, place, speed) in [(&vehicle_id, &madrid, 70), (&vehicle_2_id, &berlin, 60)] {
        let part = Entity::builder()
            .id(id.clone())
            .entity_type(vehicle_type.clone())
            .geo_property("location", place.clone())
            .build()?;
        broker.entities().create(&part).send().await?.done()?;
        let held = Entity::builder()
            .id(id.clone())
            .entity_type(vehicle_type.clone())
            .property("speed", speed)
            .build()?;
        broker_b.entities().create(&held).send().await?.done()?;
    }
    let registration = CSourceRegistration::builder(broker::peer(PeerRole::B)?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::QueryEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let page = broker
        .with_accept(Accept::any())
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .attrs(vec!["speed".parse()?])
        .geo_q(geoquery::near_max(3000, madrid))
        .split_entities(EntitySplitting::Split)
        .send()
        .await?;
    expect_that!(
        page.items(),
        unordered_elements_are![all!(
            has_id(&vehicle_id),
            has_attribute("speed", any_attribute().value(70)),
            lacks_attribute("location")
        )]
    );
    Ok(())
}
