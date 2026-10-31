//! A temporal map asks the registrations whose scope may hold what its scope query matches.

use andromeda_test_suite::{
    broker,
    context_source::ContextSource,
    namespace::Namespace,
    received_status::ExpectedStatus,
    source_answer,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, OneOrMany, Scope},
    query::{scope_query::ScopeQuery, temporal_query},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Create `EntityMap` for Query Temporal Evolution of Entities by type with a scopeQ under inclusive registrations advertising createEntityMapQueryTemporal, one in the queried scope, one in another and one without a scope, answers 201 and asks the first and the unscoped source, not the second
/// Clause: 5.14.5.4, 5.12, 4.3.6.7
/// Tenant: private (it counts every request its context sources receive, and the unscoped requests of other tests
/// reach every registration of the tenant)
#[gtest]
#[tokio::test]
async fn n_map_qte_9314_a_temporal_map_asks_the_registrations_whose_scope_may_hold_a_scope_query_match() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::private_tenant(&broker::front()?, &namespace).await?;
    let source = ContextSource::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let scope_query = format!("/{namespace}/A");
    for (number, endpoint, scope) in [
        (1, "a", Some(format!("/{namespace}/A"))),
        (2, "b", Some(format!("/{namespace}/B"))),
        (3, "c", None),
    ] {
        let registration = CSourceRegistration::builder(source.endpoint(endpoint)?)
            .id(namespace.registration_id(number)?)
            .information([RegistrationInfo::builder()
                .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
                .build()?])
            .mode(RegistrationMode::Inclusive)
            .operations([DistributedOperation::CreateEntityMapQueryTemporal])
            .maybe_scope(scope.map(|scope| scope.parse::<Scope>().map(OneOrMany::one)).transpose()?)
            .build()?;
        broker.registrations().register(&registration).send().await?;
        source
            .answer(
                Method::GET,
                &format!("/{endpoint}/ngsi-ld/v1/temporal/entityMaps"),
                source_answer::status(StatusCode::NOT_FOUND),
            )
            .await?;
    }
    broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_maps()
        .create_temporal(temporal_query::before("2022-01-01T00:00:00Z".parse()?))
        .entity_type(&vehicle_type)
        .scope_q(scope_query.parse::<ScopeQuery>()?)
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    let asked_a = source.received_matching(&SourceRequest::any_matching("/a/.*")).await?;
    expect_that!(asked_a, len(eq(1)));
    let asked_b = source.received_matching(&SourceRequest::any_matching("/b/.*")).await?;
    expect_that!(asked_b, len(eq(0)));
    // Clause 4.3.6.7: a registration without scope may hold a match, and applies the scope query itself.
    let asked_c = source.received_matching(&SourceRequest::any_matching("/c/.*")).await?;
    expect_that!(asked_c, len(eq(1)));
    let asked_c_with_scope = source
        .received_matching(&SourceRequest::any_matching("/c/.*").query("scopeQ", &scope_query))
        .await?;
    expect_that!(asked_c_with_scope, len(eq(1)));
    Ok(())
}
