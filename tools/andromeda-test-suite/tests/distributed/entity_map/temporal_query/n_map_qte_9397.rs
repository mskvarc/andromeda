//! A split temporal map asks every matching source whatever its scope.

use andromeda_test_suite::{
    broker,
    context_source::ContextSource,
    forwarded_query::query_values,
    namespace::Namespace,
    problem::answered,
    received_status::ExpectedStatus,
    source_answer,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::RegisterContextSource},
    model::{NgsiLdMediaType, RawJson},
    query::{EntitySplitting, scope_query::ScopeQuery, temporal_query},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Create `EntityMap` for Query Temporal Evolution of Entities by type with a scopeQ and `splitEntities` true under inclusive registrations advertising createEntityMapQueryTemporal, one in the queried scope, one in another and one without a scope, answers 201 and asks all three sources, none of them with the scopeQ
/// Clause: 5.14.5.4, 5.12
/// Tenant: private (it counts every request its context sources receive, and the unscoped requests of other tests
/// reach every registration of the tenant)
#[gtest]
#[tokio::test]
async fn n_map_qte_9397_a_split_temporal_map_asks_every_matching_source_whatever_its_scope() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::private_tenant(&broker::front()?, &namespace).await?;
    let source = ContextSource::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_a = CSourceRegistration::builder(source.endpoint("a")?)
        .id(namespace.registration_id(1)?)
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::CreateEntityMapQueryTemporal])
        .build()?;
    // The model refuses it (a registration scope), so `scope` is written as text.
    let scope_a: RawJson = format!(r#"["/{namespace}/A"]"#).parse()?;
    let document_a = UncheckedDocument::try_from(registration_a.render())?.set_member("scope", &scope_a);
    let request_a = broker
        .unchecked(&RegisterContextSource { registration: &registration_a })?
        .document(&document_a);
    let answer_a = broker.send_unchecked(request_a).await;
    expect_that!(&answer_a, answered(StatusCode::CREATED));
    let registration_b = CSourceRegistration::builder(source.endpoint("b")?)
        .id(namespace.registration_id(2)?)
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::CreateEntityMapQueryTemporal])
        .build()?;
    // The model refuses it (a registration scope), so `scope` is written as text.
    let scope_b: RawJson = format!(r#"["/{namespace}/B"]"#).parse()?;
    let document_b = UncheckedDocument::try_from(registration_b.render())?.set_member("scope", &scope_b);
    let request_b = broker
        .unchecked(&RegisterContextSource { registration: &registration_b })?
        .document(&document_b);
    let answer_b = broker.send_unchecked(request_b).await;
    expect_that!(&answer_b, answered(StatusCode::CREATED));
    let registration_c = CSourceRegistration::builder(source.endpoint("c")?)
        .id(namespace.registration_id(3)?)
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::CreateEntityMapQueryTemporal])
        .build()?;
    broker.registrations().register(&registration_c).send().await?;
    source
        .answer(Method::GET, "/a/ngsi-ld/v1/temporal/entityMaps", source_answer::status(StatusCode::NOT_FOUND))
        .await?;
    source
        .answer(Method::GET, "/b/ngsi-ld/v1/temporal/entityMaps", source_answer::status(StatusCode::NOT_FOUND))
        .await?;
    source
        .answer(Method::GET, "/c/ngsi-ld/v1/temporal/entityMaps", source_answer::status(StatusCode::NOT_FOUND))
        .await?;
    broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_maps()
        .create_temporal(temporal_query::before("2022-01-01T00:00:00Z".parse()?))
        .entity_type(&vehicle_type)
        .scope_q(format!("/{namespace}/A").parse::<ScopeQuery>()?)
        .split_entities(EntitySplitting::Split)
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    let asked_a = source.received_matching(&SourceRequest::any_matching("/a/.*")).await?;
    expect_that!(asked_a, len(eq(1)));
    let asked_a_one = asked_a.first().or_fail()?;
    expect_that!(query_values(asked_a_one, "scopeQ"), is_empty());
    let asked_b = source.received_matching(&SourceRequest::any_matching("/b/.*")).await?;
    expect_that!(asked_b, len(eq(1)));
    let asked_b_one = asked_b.first().or_fail()?;
    expect_that!(query_values(asked_b_one, "scopeQ"), is_empty());
    let asked_c = source.received_matching(&SourceRequest::any_matching("/c/.*")).await?;
    expect_that!(asked_c, len(eq(1)));
    let asked_c_one = asked_c.first().or_fail()?;
    expect_that!(query_values(asked_c_one, "scopeQ"), is_empty());
    Ok(())
}
