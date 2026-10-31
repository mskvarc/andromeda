//! An exclusive registration looping back yields a loop warning.

use andromeda_test_suite::{
    answer_warning::warns_with,
    broker,
    config::{BrokerRole, PeerRole},
    namespace::Namespace,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::{Accept, WarningCode},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Retrieve Entity on broker A of an Entity whose speed is registered exclusively at broker B, which
/// registers it exclusively back at A, carries NGSILD-Warning code 199
/// Clause: 6.3.17, 6.3.18, 4.3.6.4
#[gtest]
#[tokio::test]
async fn n_fed_err_6400_an_exclusive_registration_looping_back_yields_a_loop_warning() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let broker_b = broker::client(BrokerRole::B)?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_id = namespace.registration_id(1)?;
    let registration_2_id = namespace.registration_id(2)?;
    let registration = CSourceRegistration::builder(broker::peer(PeerRole::B)?)
        .id(registration_id)
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_id.clone())])
            .property_names(["speed"])
            .build()?])
        .mode(RegistrationMode::Exclusive)
        .operations([DistributedOperation::RetrieveEntity, DistributedOperation::QueryEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    let registration_2 = CSourceRegistration::builder(broker::peer(PeerRole::A)?)
        .id(registration_2_id)
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type).with_id(vehicle_id.clone())])
            .property_names(["speed"])
            .build()?])
        .mode(RegistrationMode::Exclusive)
        .operations([DistributedOperation::RetrieveEntity, DistributedOperation::QueryEntity])
        .build()?;
    broker_b.registrations().register(&registration_2).send().await?;
    let answer = broker.with_responses().with_accept(Accept::any()).entities().retrieve(&vehicle_id).send().await;
    expect_that!(&answer, warns_with(WarningCode::MiscellaneousWarning));
    Ok(())
}
