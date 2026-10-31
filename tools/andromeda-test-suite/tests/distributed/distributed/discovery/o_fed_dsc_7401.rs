//! A promised type is described with its type name.

use andromeda_test_suite::{
    broker,
    config::{BrokerRole, PeerRole},
    namespace::Namespace,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::Entity,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo},
};

/// Pins: Retrieve Available Entity Type Information of a type only a registration of broker B promises, with two
/// Aircraft held by broker B, answers 200 with an `EntityTypeInfo` naming the type
/// Clause: 5.7.7, 5.7.8
/// Tenant: private (a type or attribute discovery request is forwarded to every source of the tenant supporting the
/// operation, clause 5.7.11, so every other file's registration would see it)
/// Recorded: clause 5.7.11 lets an implementation answer discovery from local data and the registry alone, or
/// forward to every source supporting the operation and merge the answers; the request is answered 200 with the
/// registered type or attribute either way
#[gtest]
#[tokio::test]
async fn o_fed_dsc_7401_a_promised_type_is_described_with_its_type_name() -> Result<()> {
    let namespace = Namespace::fresh();
    let tenant = broker::private_tenant(&broker::front()?, &namespace).await?;
    let broker_b = broker::client(BrokerRole::B)?.with_tenant(namespace.tenant()?);
    let aircraft_id = namespace.urn("Aircraft", 1)?;
    let aircraft_type = namespace.entity_type("Aircraft")?;
    let aircraft_2_id = namespace.urn("Aircraft", 2)?;
    let registration_id = namespace.registration_id(1)?;
    let aircraft = Entity::builder()
        .id(aircraft_id)
        .entity_type(aircraft_type.clone())
        .property("thrust", 1000)
        .build()?;
    broker_b.entities().create(&aircraft).send().await?.done()?;
    let aircraft_2 = Entity::builder()
        .id(aircraft_2_id)
        .entity_type(aircraft_type.clone())
        .property("thrust", 1000)
        .build()?;
    broker_b.entities().create(&aircraft_2).send().await?.done()?;
    let registration = CSourceRegistration::builder(broker::peer(PeerRole::B)?)
        .id(registration_id)
        .information([RegistrationInfo::builder().entities([EntityInfo::new(aircraft_type.clone())]).build()?])
        .operations([DistributedOperation::RetrieveEntity, DistributedOperation::QueryEntity])
        .tenant(namespace.tenant()?)
        .build()?;
    tenant.registrations().register(&registration).send().await?;
    let information = tenant.with_accept(Accept::any()).entity_types().retrieve(&aircraft_type).send().await?;
    expect_that!(information.type_name, eq(&aircraft_type));
    Ok(())
}
