//! A registration naming broker A and leading back into the tenant is never contacted.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    identity_matcher::has_id,
    namespace::Namespace,
    outcome::Done,
    source_answer,
};
use googletest::prelude::*;
use http::Method;
use ngsi_ld::{
    http::headers::{Accept, NgsiLdWarning, WarningCode},
    model::{ContextSourceAlias, Entity},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Query Entities by type in a named tenant of broker A, under an inclusive registration whose
/// `contextSourceAlias` is broker A's configured alias and whose `tenant` is that same tenant, answers 200 with the
/// local Entity and one NGSILD-Warning code 199, and sends the registration's endpoint no request
/// Clause: 4.3.6.4, 4.14, 6.3.17, 5.7.2.4
/// Tenant: private (the loop exists only when the registration's `tenant` is the tenant the query runs in)
#[gtest]
#[tokio::test]
async fn n_fed_qry_7608_a_registration_naming_broker_a_and_leading_back_into_the_tenant_is_never_contacted() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::private_tenant(&broker::front()?, &namespace).await?;
    let source = ContextSource::start().await?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 5)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let source_entities = [];
    source
        .answer(Method::GET, "/ngsi-ld/v1/entities", source_answer::entities(&source_entities))
        .await?;
    // The endpoint is a recording source rather than broker A itself: the alias alone names broker A, and a request
    // that left would be seen there.
    let registration = CSourceRegistration::builder(source.root_endpoint()?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::QueryEntity])
        .context_source_alias("broker-a".parse::<ContextSourceAlias>()?)
        .tenant(namespace.tenant()?)
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let page = broker.with_accept(Accept::any()).entities().query().entity_type(&vehicle_type).send().await?;
    expect_that!(page.items(), elements_are![has_id(&vehicle_id)]);
    expect_that!(
        page.warnings()
            .valid()
            .map(|warnings| warnings.iter().map(NgsiLdWarning::code).collect::<Vec<_>>()),
        some(elements_are![eq(&WarningCode::MiscellaneousWarning)])
    );
    expect_that!(source.received_on(Method::GET, "/ngsi-ld/v1/entities").await?, len(eq(0)));
    Ok(())
}
