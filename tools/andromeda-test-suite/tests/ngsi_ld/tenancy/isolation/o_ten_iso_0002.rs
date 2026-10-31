//! An entity is not reached through a tenant that does not exist.

use andromeda_test_suite::{broker, cleanup::Cleanup, config::SuiteConfig, namespace::Namespace, outcome::Done, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, NgsiLdMediaType},
    registration::{CSourceRegistration, RegistrationInfo},
};

/// Pins: an entity created in one Tenant is not reached by its id through another Tenant that does not exist: the
/// request answers 404 `NonexistentTenant`
/// Clause: 4.14, 5.5.10, 5.7.1.4
/// Tenant: private (the subject is one Tenant's entity seen from another)
/// Recorded: clause 5.5.10 makes implicit Tenant creation an implementation option and says the other operations on
/// a non-existing Tenant "should" raise `NonexistentTenant`; Andromeda creates Tenants implicitly, exactly on the
/// six operations the clause lists
#[gtest]
#[tokio::test]
async fn o_ten_iso_0002_an_entity_is_not_reached_through_a_tenant_that_does_not_exist() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let tenant = broker.with_tenant(namespace.tenant()?);
    let registration_provision_id = namespace.registration_id("provision")?;
    let provisioning_sentinel_type = namespace.entity_type("ProvisioningSentinel")?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let b_tenant = broker.with_tenant(format!("{namespace}-b").parse()?);
    let registration_provision = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_provision_id.clone())
        .information([RegistrationInfo::builder().entities([provisioning_sentinel_type.as_str()]).build()?])
        .build()?;
    tenant.registrations().register(&registration_provision).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_provision_cleanup = Cleanup::of(&tenant).registration(&registration_provision_id);
    tenant.registrations().delete(&registration_provision_id).send().await?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("name", "tenant-a")
        .build()?;
    tenant.entities().create(&vehicle).send().await?.done()?;
    let answer = b_tenant
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::NonexistentTenant));
    Ok(())
}
