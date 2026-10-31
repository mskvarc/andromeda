//! A lone redirect source that answers yields 201.

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

/// Pins: Create `EntityMap` for Query Entities matching only a redirect registration whose source answers an
/// `EntityMap` answers 201 with an `NGSILD-EntityMap` header
/// Clause: 5.14.4, Table 6.34.3.1-1
/// Tenant: private (the broker forwards an entity map query to the redirect and inclusive registrations of other
/// tests in the same tenant)
#[gtest]
#[tokio::test]
async fn n_map_qen_9006_a_lone_redirect_source_that_answers_yields_201() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::private_tenant(&broker::front()?, &namespace).await?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let map_declined_probe_type = namespace.entity_type("MapDeclinedProbe")?;
    let remote_map = EntityMap::new(namespace.urn("EntityMap", "remote")?.to_string().parse()?, "2099-01-01T00:00:00Z".parse()?);
    source
        .answer(
            Method::GET,
            "/a/ngsi-ld/v1/entityMaps",
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
    let created = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_maps()
        .create()
        .entity_type(&map_declined_probe_type)
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    expect_that!(created.entity_map(), present());
    let forwarded = source.received_on(Method::GET, "/a/ngsi-ld/v1/entityMaps").await?;
    expect_that!(forwarded, len(eq(1)));
    broker.registrations().delete(&registration_id).send().await?;
    Ok(())
}
