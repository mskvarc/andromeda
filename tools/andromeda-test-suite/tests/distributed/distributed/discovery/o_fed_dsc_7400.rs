//! A type only a registration promises is listed.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    config::{PeerRole, SuiteConfig},
    namespace::Namespace,
    problem::answered,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::RegisterContextSource},
    model::RawJson,
    registration::{CSourceRegistration, DistributedOperation, RegistrationInfo},
};

/// Pins: Retrieve Available Entity Types lists the type that only a registration of broker B promises, held neither
/// locally nor yet by broker B
/// Clause: 5.7.5, 5.7.6
/// Tenant: private (a type or attribute discovery request is forwarded to every source of the tenant supporting the
/// operation, clause 5.7.11, so every other file's registration would see it)
/// Recorded: clause 5.7.11 lets an implementation answer discovery from local data and the registry alone, or
/// forward to every source supporting the operation and merge the answers; the request is answered 200 with the
/// registered type or attribute either way
#[gtest]
#[tokio::test]
async fn o_fed_dsc_7400_a_type_only_a_registration_promises_is_listed() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let tenant = broker.with_tenant(namespace.tenant()?);
    let registration_provision_id = namespace.registration_id("provision")?;
    let provisioning_sentinel_type = namespace.entity_type("ProvisioningSentinel")?;
    let registration_id = namespace.registration_id(1)?;
    let aircraft_type = namespace.entity_type("Aircraft")?;
    let registration_provision = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_provision_id.clone())
        .information([RegistrationInfo::builder().entities([provisioning_sentinel_type.as_str()]).build()?])
        .build()?;
    tenant.registrations().register(&registration_provision).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_provision_cleanup = Cleanup::of(&tenant).registration(&registration_provision_id);
    tenant.registrations().delete(&registration_provision_id).send().await?;
    let registration = CSourceRegistration::builder(broker::peer(PeerRole::B)?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder().entities([aircraft_type.as_str()]).build()?])
        .operations([DistributedOperation::RetrieveEntity, DistributedOperation::QueryEntity])
        .build()?;
    // The model refuses it (a registration tenant), so `tenant` is written as text.
    let tenant_2: RawJson = format!(r#""{namespace}""#).parse()?;
    let document = UncheckedDocument::try_from(registration.render())?.set_member("tenant", &tenant_2);
    let request = tenant.unchecked(&RegisterContextSource { registration: &registration })?.document(&document);
    let answer = tenant.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let _registration_cleanup = Cleanup::of(&tenant).registration(&registration_id);
    let types = tenant.with_accept(Accept::any()).entity_types().list().send().await?;
    expect_that!(types.type_list, contains(eq(&aircraft_type)));
    Ok(())
}
