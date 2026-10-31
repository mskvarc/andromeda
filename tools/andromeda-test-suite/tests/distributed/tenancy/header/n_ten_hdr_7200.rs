//! A request naming a tenant is answered with the same tenant header on every success status.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    cleanup::Cleanup,
    config::SuiteConfig,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::{Accept, NgsiLdTenant},
    model::Entity,
    registration::{CSourceRegistration, RegistrationInfo},
};

/// Pins: Create Entity, Retrieve Entity and Delete Entity carrying the NGSILD-Tenant header are answered 201, 200
/// and 204 each with the same NGSILD-Tenant header
/// Clause: 6.3.14, 4.14
/// Tenant: private (the subject is the response header of a named Tenant)
#[gtest]
#[tokio::test]
async fn n_ten_hdr_7200_a_request_naming_a_tenant_is_answered_with_the_same_tenant_header_on_every_success_status() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let tenant = broker.with_tenant(namespace.tenant()?);
    let registration_provision_id = namespace.registration_id("provision")?;
    let provisioning_sentinel_type = namespace.entity_type("ProvisioningSentinel")?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
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
        .property("speed", 5)
        .build()?;
    let received = tenant.with_responses().entities().create(&vehicle).send().await?;
    received.outcome.done()?;
    expect_that!(
        received.head.typed::<NgsiLdTenant>().valid().map(NgsiLdTenant::tenant),
        some(eq(&namespace.tenant()?))
    );
    let headed = tenant.with_heads().with_accept(Accept::any()).entities().retrieve(&vehicle_id).send().await?;
    expect_that!(
        headed.head.typed::<NgsiLdTenant>().valid().map(NgsiLdTenant::tenant),
        some(eq(&namespace.tenant()?))
    );
    let retrieved = &headed.outcome;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("speed", any_attribute().value(5)));
    let received_2 = tenant.with_responses().entities().delete(&vehicle_id).send().await?;
    received_2.outcome.done()?;
    expect_that!(
        received_2.head.typed::<NgsiLdTenant>().valid().map(NgsiLdTenant::tenant),
        some(eq(&namespace.tenant()?))
    );
    Ok(())
}
