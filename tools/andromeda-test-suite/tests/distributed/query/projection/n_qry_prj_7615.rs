//! A split query decides q on the merged entity before an omit removes the attribute q reads.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    cleanup::Cleanup,
    config::BrokerRole,
    entity_matcher::{has_attribute, lacks_attribute},
    identity_matcher::has_id,
    namespace::Namespace,
    outcome::Done,
    peer_role::PeerRole,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
    query::{EntitySplitting, projection::Projection, q},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Query Entities by type with `splitEntities` true, omit brand and q brand=="Mercedes", over an inclusive
/// registration of peer broker B, answers the Entity whose local part holds only brand Mercedes and whose speed 70 B
/// holds, without brand, and not the Entity whose local part holds only brand Volvo and whose speed 60 B holds
/// Clause: 5.7.2.4, 5.7.2.5, 4.21, 4.3.6
#[gtest]
#[tokio::test]
async fn n_qry_prj_7615_a_split_query_decides_q_on_the_merged_entity_before_an_omit_removes_what_q_reads() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let broker_b = broker::client(BrokerRole::B)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_2_id = namespace.urn("Vehicle", 2)?;
    let registration_id = namespace.registration_id(1)?;
    for (id, brand, speed) in [(&vehicle_id, "Mercedes", 70), (&vehicle_2_id, "Volvo", 60)] {
        let part = Entity::builder()
            .id(id.clone())
            .entity_type(vehicle_type.clone())
            .property("brand", brand)
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
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query_projected()
        .entity_type(&vehicle_type)
        .omit("brand".parse::<Projection>()?)
        .q(q::attr("brand").eq("Mercedes"))
        .split_entities(EntitySplitting::Split)
        .send()
        .await?;
    expect_that!(
        page.items(),
        unordered_elements_are![all!(
            has_id(&vehicle_id),
            has_attribute("speed", any_attribute().value(70)),
            lacks_attribute("brand")
        )]
    );
    Ok(())
}
