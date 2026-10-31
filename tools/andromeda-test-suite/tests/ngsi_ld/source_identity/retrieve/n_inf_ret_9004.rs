//! Retrieve context source identity echoes the tenant header.

use andromeda_test_suite::{broker, cleanup::Cleanup, config::SuiteConfig, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::{Accept, NgsiLdTenant},
    model::NgsiLdMediaType,
    registration::{CSourceRegistration, RegistrationInfo},
};

/// Pins: Retrieve Context Source Identity for a named tenant answers 200 with that tenant in the NGSILD-Tenant
/// header
/// Clause: 6.3.14, 5.15.1.4
/// Tenant: private (the subject is how a named tenant identifies itself; the files name two tenants of their own,
/// which a registration created and deleted brings into existence)
#[gtest]
#[tokio::test]
async fn n_inf_ret_9004_retrieve_context_source_identity_echoes_the_tenant_header() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let tenant = broker.with_tenant(namespace.tenant()?);
    let registration_provision_id = namespace.registration_id("provision")?;
    let sentinel_type = namespace.entity_type("Sentinel")?;
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
    let headed = tenant
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .source_identity()
        .send()
        .await?;
    expect_that!(
        headed.head.typed::<NgsiLdTenant>().valid().map(NgsiLdTenant::tenant),
        some(eq(&namespace.tenant()?))
    );
    Ok(())
}
