//! A promised attribute is described with its attribute name.

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

/// Pins: Retrieve Available Attribute Information of an attribute that only a registration of broker B promises,
/// with an Aircraft holding it at broker B, answers 200 with an Attribute naming it
/// Clause: 5.7.11, 5.7.12
/// Tenant: private (a type or attribute discovery request is forwarded to every source of the tenant supporting the
/// operation, clause 5.7.11, so every other file's registration would see it)
/// Recorded: clause 5.7.11 lets an implementation answer discovery from local data and the registry alone, or
/// forward to every source supporting the operation and merge the answers; the request is answered 200 with the
/// registered type or attribute either way
#[gtest]
#[tokio::test]
async fn o_fed_dsc_7403_a_promised_attribute_is_described_with_its_attribute_name() -> Result<()> {
    let namespace = Namespace::fresh();
    let tenant = broker::private_tenant(&broker::front()?, &namespace).await?;
    let broker_b = broker::client(BrokerRole::B)?.with_tenant(namespace.tenant()?);
    let aircraft_id = namespace.urn("Aircraft", 1)?;
    let aircraft_type = namespace.entity_type("Aircraft")?;
    let registration_id = namespace.registration_id(1)?;
    let thrust = namespace.attribute("thrust")?;
    let aircraft = Entity::builder()
        .id(aircraft_id)
        .entity_type(aircraft_type.clone())
        .property(thrust.to_string().as_str(), 1000)
        .build()?;
    broker_b.entities().create(&aircraft).send().await?.done()?;
    let registration = CSourceRegistration::builder(broker::peer(PeerRole::B)?)
        .id(registration_id)
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(aircraft_type)])
            .property_names([thrust.to_string()])
            .build()?])
        .operations([DistributedOperation::RetrieveEntity, DistributedOperation::QueryEntity])
        .tenant(namespace.tenant()?)
        .build()?;
    tenant.registrations().register(&registration).send().await?;
    let information = tenant.with_accept(Accept::any()).attributes().retrieve(&thrust).send().await?;
    expect_that!(information.attribute_name, eq(&thrust));
    Ok(())
}
