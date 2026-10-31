//! A lone redirect source 501 is relayed by the temporal resource.

use andromeda_test_suite::{broker, cleanup::Cleanup, context_source::ContextSource, namespace::Namespace, problem::problem, source_answer};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType},
    query::{IdPattern, temporal_query},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Create `EntityMap` for Query Temporal Evolution of Entities matching only a redirect registration whose
/// source answers 501 answers 501 with a `ProblemDetails` of type `OperationNotSupported`
/// Clause: 5.14.5, 6.3.7, 6.3.17
#[gtest]
#[tokio::test]
async fn o_map_qte_9001_a_lone_redirect_source_501_is_relayed_by_the_temporal_resource() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let map_declined_probe_type = namespace.entity_type("MapDeclinedProbe")?;
    source
        .answer(
            Method::GET,
            "/a/ngsi-ld/v1/temporal/entityMaps",
            source_answer::status(StatusCode::NOT_IMPLEMENTED),
        )
        .await?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(map_declined_probe_type.clone()).with_id_pattern(format!("urn:ngsi-ld:.+:{namespace}-.*").parse::<IdPattern>()?)])
            .build()?])
        .mode(RegistrationMode::Redirect)
        .operations([DistributedOperation::CreateEntityMapQueryTemporal])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    // Table 6.35.3.1-1 names no 501, but neither does it name the 203 clause 4.3.6.8 requires; a 201 would answer a map omitting the source holding all of the data.
    let answer = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_maps()
        .create_temporal(temporal_query::after("2020-01-01T00:00:00Z".parse()?))
        .entity_type(&map_declined_probe_type)
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::NOT_IMPLEMENTED, ErrorType::OperationNotSupported).with_title());
    let forwarded = source.received_on(Method::GET, "/a/ngsi-ld/v1/temporal/entityMaps").await?;
    expect_that!(forwarded, len(eq(1)));
    broker.registrations().delete(&registration_id).send().await?;
    Ok(())
}
