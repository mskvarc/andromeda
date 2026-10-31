//! A tenant holding no entity lists nothing.

use andromeda_test_suite::{broker, cleanup::Cleanup, config::SuiteConfig, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    registration::{CSourceRegistration, RegistrationInfo},
};

/// Pins: Retrieve Available Attributes in a tenant where no entity exists answers an `AttributeList` whose
/// `attributeList` is empty
/// Clause: 5.7.8.4, 5.2.27, 6.27.3.1
/// Tenant: private (the subject is the completeness of the unfiltered attribute list, empty only in a tenant no
/// other file writes to)
#[gtest]
#[tokio::test]
async fn n_dsc_att_0001_a_tenant_holding_no_entity_lists_nothing() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let tenant = broker.with_tenant(namespace.tenant()?);
    let registration_provision_id = namespace.registration_id("provision")?;
    let provisioning_sentinel_type = namespace.entity_type("ProvisioningSentinel")?;
    let registration_provision = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_provision_id.clone())
        .information([RegistrationInfo::builder().entities([provisioning_sentinel_type.as_str()]).build()?])
        .build()?;
    tenant.registrations().register(&registration_provision).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_provision_cleanup = Cleanup::of(&tenant).registration(&registration_provision_id);
    tenant.registrations().delete(&registration_provision_id).send().await?;
    let attributes = tenant.with_accept(Accept::any()).attributes().list().send().await?;
    expect_that!(attributes.attribute_list, len(eq(0)));
    Ok(())
}
