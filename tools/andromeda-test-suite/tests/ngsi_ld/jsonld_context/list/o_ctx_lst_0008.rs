//! List a fresh tenant details false kind implicitly created answers no contexts.

use andromeda_test_suite::{broker, cleanup::Cleanup, config::SuiteConfig, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::{
    http::{
        headers::Accept,
        operation::{ContextKindFilter, SummaryDetails},
    },
    registration::{CSourceRegistration, RegistrationInfo},
};

/// Pins: List @contexts with details=false and `kind=ImplicitlyCreated` in a tenant that has stored nothing answers
/// 200 with an empty list
/// Clause: 5.13.3.4, 6.29.3.2
/// Tenant: private (the case asserts what the tenant's whole @context store lists, and clause 4.14 keeps each
/// tenant's stored @contexts its own; in the shared tenant every other file's entries are listed too)
/// Recorded: clause 5.13.3.4 lists "@contexts currently stored"; the core @context is not a stored entry, so a
/// tenant that has stored nothing lists nothing
#[gtest]
#[tokio::test]
async fn o_ctx_lst_0008_list_a_fresh_tenant_details_false_kind_implicitly_created_answers_no_contexts() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let tenant = broker.with_tenant(namespace.tenant()?);
    let registration_provision_id = namespace.registration_id("provision")?;
    let sentinel_type = namespace.entity_type("Sentinel")?;
    let registration_provision = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_provision_id.clone())
        .information([RegistrationInfo::builder().entities([sentinel_type.as_str()]).build()?])
        .build()?;
    tenant.registrations().register(&registration_provision).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_provision_cleanup = Cleanup::of(&tenant).registration(&registration_provision_id);
    tenant.registrations().delete(&registration_provision_id).send().await?;
    let contexts = tenant
        .with_accept(Accept::any())
        .contexts()
        .list()
        .kind(ContextKindFilter::ImplicitlyCreated)
        .details(SummaryDetails::Explicit)
        .send()
        .await?;
    expect_that!(contexts, len(eq(0)));
    Ok(())
}
