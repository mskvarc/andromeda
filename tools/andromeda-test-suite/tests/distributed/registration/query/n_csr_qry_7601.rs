//! An attribute list matches a registered relationship name.

use andromeda_test_suite::{broker, cleanup::Cleanup, config::PeerRole, identity_matcher::has_id, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
};

/// Pins: Query Context Source Registrations by type and attrs naming a Relationship that a registration created on
/// broker A lists in `relationshipNames` returns that registration
/// Clause: 5.10.2.4, 5.12
#[gtest]
#[tokio::test]
async fn n_csr_qry_7601_an_attribute_list_matches_a_registered_relationship_name() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration = CSourceRegistration::builder(broker::peer(PeerRole::B)?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern_ending("Vehicle", "1-.*")?)])
            .relationship_names([namespace.attribute("parkedIn")?.to_string()])
            .build()?])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let page = broker
        .with_accept(Accept::any())
        .registrations()
        .query()
        .entity_type(&vehicle_type)
        .attrs(vec![format!("parkedIn_{namespace}").parse()?])
        .send()
        .await?;
    expect_that!(page.items(), contains(has_id(&registration_id)));
    let page_2 = broker
        .with_accept(Accept::any())
        .registrations()
        .query()
        .entity_type(&vehicle_type)
        .attrs(vec![format!("parkedIn_{namespace}").parse()?])
        .send()
        .await?;
    expect_that!(page_2.items(), contains(has_id(&registration_id)));
    Ok(())
}
