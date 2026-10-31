//! A lone redirect source 501 is relayed.

use andromeda_test_suite::{broker, cleanup::Cleanup, context_source::ContextSource, namespace::Namespace, problem::problem, source_answer};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType},
    query::IdPattern,
    registration::{CSourceRegistration, EntityInfo, OperationGroup, RegistrationInfo, RegistrationMode},
};

/// Pins: Create `EntityMap` for Query Entities matching only a redirect registration whose source answers the
/// forwarded creation with 501 answers 501 with a `ProblemDetails` of type `OperationNotSupported`
/// Clause: 5.14.4, Table 6.34.3.1-1, 6.3.7, 6.3.17
/// Tenant: private (the broker forwards an entity map query to the redirect and inclusive registrations of other
/// tests in the same tenant)
#[gtest]
#[tokio::test]
async fn n_map_qen_9001_a_lone_redirect_source_501_is_relayed() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::private_tenant(&broker::front()?, &namespace).await?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let map_declined_probe_type = namespace.entity_type("MapDeclinedProbe")?;
    source
        .answer(Method::GET, "/a/ngsi-ld/v1/entityMaps", source_answer::status(StatusCode::NOT_IMPLEMENTED))
        .await?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(map_declined_probe_type.clone()).with_id_pattern(format!("urn:ngsi-ld:.+:{namespace}-.*").parse::<IdPattern>()?)])
            .build()?])
        .mode(RegistrationMode::Redirect)
        .operations([OperationGroup::RedirectionOps])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let answer = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_maps()
        .create()
        .entity_type(&map_declined_probe_type)
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::NOT_IMPLEMENTED, ErrorType::OperationNotSupported).with_title());
    let forwarded = source.received_on(Method::GET, "/a/ngsi-ld/v1/entityMaps").await?;
    expect_that!(forwarded, len(eq(1)));
    broker.registrations().delete(&registration_id).send().await?;
    Ok(())
}
