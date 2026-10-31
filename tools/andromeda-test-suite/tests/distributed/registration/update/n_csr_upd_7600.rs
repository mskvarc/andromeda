//! A fragment is merged and reads back.

use andromeda_test_suite::{broker, cleanup::Cleanup, config::PeerRole, namespace::Namespace, problem::answered};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    registration::{CSourceRegistration, EntityInfo, RegistrationFragment, RegistrationInfo, RegistrationTypeMember},
};

/// Pins: Update Context Source Registration on broker A of a registration naming broker B with a description
/// fragment answers 204 with no body, and the description reads back
/// Clause: 5.9.3, 5.10.1, 5.5.8
#[gtest]
#[tokio::test]
async fn n_csr_upd_7600_a_fragment_is_merged_and_reads_back() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration = CSourceRegistration::builder(broker::peer(PeerRole::B)?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let fragment = RegistrationFragment::builder()
        .description("updated telemetry source")
        .type_member(RegistrationTypeMember::Omitted)
        .build()?;
    let answer = broker.with_responses().registrations().update(&registration_id, &fragment).send().await;
    expect_that!(&answer, answered(StatusCode::NO_CONTENT).without_body());
    let registration_2 = broker.with_accept(Accept::any()).registrations().retrieve(&registration_id).send().await?;
    expect_that!(registration_2.description(), some(eq("updated telemetry source")));
    let registration_3 = broker.with_accept(Accept::any()).registrations().retrieve(&registration_id).send().await?;
    expect_that!(registration_3.description(), some(eq("updated telemetry source")));
    Ok(())
}
