//! A version preference amends peer data with 203.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    cleanup::Cleanup,
    config::{BrokerRole, PeerRole},
    entity_matcher::has_attribute,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    received_status::ExpectedStatus,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::{Accept, Prefer, SpecVersion},
    model::Entity,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Query Entities and Retrieve Entity on broker A with Prefer ngsi-ld=1.8, over an Entity broker B serves
/// through an exclusive registration, answer 203 with Preference-Applied ngsi-ld=1.8
/// Clause: 5.7.1.4, 5.7.2.4, 4.3.6.8, 6.4.3.2
#[gtest]
#[tokio::test]
async fn n_fed_ret_7604_a_version_preference_amends_peer_data_with_203() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let broker_b = broker::client(BrokerRole::B)?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 5)
        .build()?;
    broker_b.entities().create(&vehicle).send().await?.done()?;
    let registration = CSourceRegistration::builder(broker::peer(PeerRole::B)?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_id.clone())])
            .property_names(["speed"])
            .build()?])
        .mode(RegistrationMode::Exclusive)
        .operations([DistributedOperation::RetrieveEntity, DistributedOperation::QueryEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let page = broker
        .with_heads()
        .with_prefer(Prefer::builder().version("1.8".parse::<SpecVersion>()?).build()?)
        .with_accept(Accept::any())
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .send()
        .await?
        .expecting(StatusCode::NON_AUTHORITATIVE_INFORMATION)?;
    expect_that!(page.preference_applied(), some(eq(&"1.8".parse::<SpecVersion>()?)));
    expect_that!(page.items(), nth(0, has_attribute("speed", any_attribute().value(5))));
    let retrieved = broker
        .with_heads()
        .with_prefer(Prefer::builder().version("1.8".parse::<SpecVersion>()?).build()?)
        .with_accept(Accept::any())
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await?
        .expecting(StatusCode::NON_AUTHORITATIVE_INFORMATION)?;
    expect_that!(retrieved.preference_applied(), some(eq(&"1.8".parse::<SpecVersion>()?)));
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("speed", any_attribute().value(5)));
    Ok(())
}
