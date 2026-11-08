//! A tenant named default is not the default tenant.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    header_matcher::absent,
    namespace::Namespace,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::{Accept, NgsiLdTenant},
    model::{Entity, NgsiLdMediaType, Tenant},
};

/// Pins: an entity created without NGSILD-Tenant and another with the same id created under NGSILD-Tenant default
/// are two entities: each reads back with its own value from its own Tenant, and only the named one is answered
/// with the header
/// Clause: 4.14, 5.5.10, 6.3.14
/// Tenant: private (the subject is the Tenant literally named default)
/// Recorded: clause 4.14 says an operation naming no Tenant targets the default Tenant and clause 6.3.14 omits the
/// header for it, but neither says whether the default Tenant has a name a request could give; Andromeda treats a
/// Tenant named default as a named Tenant like any other
#[gtest]
#[tokio::test]
async fn o_ten_iso_0001_a_tenant_named_default_is_not_the_default_tenant() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    broker::private_tenant(&broker, &namespace).await?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let default_tenant = broker.with_tenant("default".parse()?);
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("name", "unnamed-default")
        .build()?;
    let received = broker.with_responses().entities().create(&vehicle).send().await?;
    received.outcome.done()?;
    expect_that!(received.head.typed::<NgsiLdTenant>(), absent());
    let vehicle_2 = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("name", "named-default")
        .build()?;
    let received_2 = default_tenant.with_responses().entities().create(&vehicle_2).send().await?;
    received_2.outcome.done()?;
    expect_that!(
        received_2.head.typed::<NgsiLdTenant>().valid().map(NgsiLdTenant::tenant),
        some(eq(&"default".parse::<Tenant>()?))
    );
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("name", any_attribute().value("unnamed-default")));
    let headed = default_tenant
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    expect_that!(
        headed.head.typed::<NgsiLdTenant>().valid().map(NgsiLdTenant::tenant),
        some(eq(&"default".parse::<Tenant>()?))
    );
    let retrieved_2 = &headed.outcome;
    let entity_2 = retrieved_2.body();
    expect_that!(entity_2, has_attribute("name", any_attribute().value("named-default")));
    Ok(())
}
