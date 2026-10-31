//! Each tenant observes its own context source alias.

use andromeda_test_suite::{broker, cleanup::Cleanup, config::SuiteConfig, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::NgsiLdMediaType,
    registration::{CSourceRegistration, RegistrationInfo},
};

/// Pins: Retrieve Context Source Identity answers a non-empty single-token `contextSourceAlias` for the default
/// tenant and for two named tenants, all three distinct
/// Clause: Table 5.2.40-1, 4.14, 5.15.1.4
/// Tenant: private (the subject is how a named tenant identifies itself; the files name two tenants of their own,
/// which a registration created and deleted brings into existence)
#[gtest]
#[tokio::test]
async fn n_inf_ret_9001_each_tenant_observes_its_own_context_source_alias() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let tenant = broker.with_tenant(namespace.tenant()?);
    let registration_provision_id = namespace.registration_id("provision")?;
    let sentinel_type = namespace.entity_type("Sentinel")?;
    let beta_tenant = broker.with_tenant(format!("{namespace}beta").parse()?);
    // A registration created and deleted again brings the tenant into existence and leaves nothing in it.
    let registration_provision = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_provision_id.clone())
        .information([RegistrationInfo::builder().entities([sentinel_type.as_str()]).build()?])
        .build()?;
    tenant.registrations().register(&registration_provision).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_provision_cleanup = Cleanup::of(&tenant).registration(&registration_provision_id);
    tenant.registrations().delete(&registration_provision_id).send().await?;
    let registration_provision_2 = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_provision_id.clone())
        .information([RegistrationInfo::builder().entities([sentinel_type.as_str()]).build()?])
        .build()?;
    beta_tenant.registrations().register(&registration_provision_2).send().await?;
    let _registration_provision_2_cleanup = Cleanup::of(&beta_tenant).registration(&registration_provision_id);
    beta_tenant.registrations().delete(&registration_provision_id).send().await?;
    let identity = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).source_identity().send().await?;
    let default_alias = identity.alias;
    let identity_2 = tenant.with_accept(Accept::new(NgsiLdMediaType::Json)).source_identity().send().await?;
    expect_that!(&identity_2.alias, not(eq(&default_alias)));
    let alpha_alias = identity_2.alias;
    let identity_3 = beta_tenant.with_accept(Accept::new(NgsiLdMediaType::Json)).source_identity().send().await?;
    expect_that!(&identity_3.alias, not(eq(&default_alias)));
    expect_that!(&identity_3.alias, not(eq(&alpha_alias)));
    Ok(())
}
