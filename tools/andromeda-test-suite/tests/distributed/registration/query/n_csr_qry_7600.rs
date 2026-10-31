//! An entity info id takes precedence over its id pattern.

use andromeda_test_suite::{broker, cleanup::Cleanup, config::PeerRole, identity_matcher::has_id, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
};

/// Pins: Query Context Source Registrations on broker A for type and an id matches a registration whose
/// `EntityInfo` names that id together with an `idPattern` it does not match, and finds nothing for an id only the
/// `idPattern` matches
/// Clause: 5.2.8, 5.10.2, 5.12
#[gtest]
#[tokio::test]
async fn n_csr_qry_7600_an_entity_info_id_takes_precedence_over_its_id_pattern() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_exact_id = namespace.urn("Vehicle", "EXACT")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_other1_id = namespace.urn("Vehicle", "OTHER1")?;
    let registration = CSourceRegistration::builder(broker::peer(PeerRole::B)?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone())
                .with_id(vehicle_exact_id.clone())
                .with_id_pattern(namespace.id_pattern_ending("Vehicle", "OTHER.*")?)])
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
        .ids(vec![vehicle_exact_id.clone()])
        .send()
        .await?;
    expect_that!(page.items(), contains(has_id(&registration_id)));
    let page_2 = broker
        .with_accept(Accept::any())
        .registrations()
        .query()
        .entity_type(&vehicle_type)
        .ids(vec![vehicle_other1_id])
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(0)));
    Ok(())
}
