//! A 501 among several sources is a warning.

use andromeda_test_suite::{
    broker,
    context_source::ContextSource,
    header_matcher::present,
    namespace::Namespace,
    received_status::ExpectedStatus,
    source_answer,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::NgsiLdMediaType,
    query::IdPattern,
    registration::{CSourceRegistration, EntityInfo, EntityMap, OperationGroup, RegistrationInfo, RegistrationMode},
};

/// Pins: Create `EntityMap` for Query Entities matching a redirect registration whose source answers 501 and an
/// inclusive one whose source answers an `EntityMap` answers 201 with an NGSILD-Warning, the declining source not
/// holding all of the data
/// Clause: 5.14.4, 6.3.17, Table 6.34.3.1-1
/// Tenant: private (the broker forwards an entity map query to the redirect and inclusive registrations of other
/// tests in the same tenant)
#[gtest]
#[tokio::test]
async fn n_map_qen_9003_a_501_among_several_sources_is_a_warning() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::private_tenant(&broker::front()?, &namespace).await?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let registration_id_2 = namespace.registration_id(2)?;
    let map_declined_probe_type = namespace.entity_type("MapDeclinedProbe")?;
    source
        .answer(Method::GET, "/a/ngsi-ld/v1/entityMaps", source_answer::status(StatusCode::NOT_IMPLEMENTED))
        .await?;
    let remote_map = EntityMap::new(namespace.urn("EntityMap", "remote")?.to_string().parse()?, "2099-01-01T00:00:00Z".parse()?);
    source
        .answer(
            Method::GET,
            "/b/ngsi-ld/v1/entityMaps",
            source_answer::entity_map(StatusCode::CREATED, &remote_map),
        )
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
    let registration_2 = CSourceRegistration::builder(source.endpoint("b")?)
        .id(registration_id_2.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(map_declined_probe_type.clone()).with_id_pattern(format!("urn:ngsi-ld:.+:{namespace}-.*").parse::<IdPattern>()?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([OperationGroup::FederationOps])
        .build()?;
    broker.registrations().register(&registration_2).send().await?;
    let created = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_maps()
        .create()
        .entity_type(&map_declined_probe_type)
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    expect_that!(created.warnings(), present());
    expect_that!(created.entity_map(), present());
    let declined = source.received_on(Method::GET, "/a/ngsi-ld/v1/entityMaps").await?;
    expect_that!(declined, len(eq(1)));
    let answered = source.received_on(Method::GET, "/b/ngsi-ld/v1/entityMaps").await?;
    expect_that!(answered, len(eq(1)));
    broker.registrations().delete(&registration_id).send().await?;
    broker.registrations().delete(&registration_id_2).send().await?;
    Ok(())
}
