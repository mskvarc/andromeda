//! A registration without mode and operations reads back inclusive and federation ops.

use andromeda_test_suite::{broker, cleanup::Cleanup, config::PeerRole, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo, RegistrationMode, SupportedOperation},
};

/// Pins: Create Context Source Registration on broker A naming broker B, with no mode and no operations, answers
/// 201, and the registration reads back with mode inclusive and operations `federationOps`
/// Clause: 5.9.2, 5.10.1, 5.2.9, 4.20
/// Recorded: Table 5.2.9-1 gives inclusive and `federationOps` as the values of an undefined mode and operations
/// but does not say an undefined member is rendered; Andromeda renders both
#[gtest]
#[tokio::test]
async fn o_csr_cre_7601_a_registration_without_mode_and_operations_reads_back_inclusive_and_federation_ops() -> Result<()> {
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
    let registration_2 = broker.with_accept(Accept::any()).registrations().retrieve(&registration_id).send().await?;
    expect_that!(registration_2.mode(), some(eq(RegistrationMode::Inclusive)));
    expect_that!(
        registration_2.operations().and_then(|operations| operations.first()),
        some(eq(&"federationOps".parse::<SupportedOperation>()?))
    );
    expect_that!(registration_2.operations().map(<[_]>::len), some(eq(1)));
    Ok(())
}
