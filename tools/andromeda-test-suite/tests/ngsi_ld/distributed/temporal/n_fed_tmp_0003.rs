//! With split entities a temporal scope q is applied after the merge and not forwarded.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    forwarded_query::query_values,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    problem::answered,
    source_answer,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode};
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::CreateOrUpdateTemporalEvolution},
    model::{Lossless, Property, RawJson, evolution::TemporalEntity},
    query::{EntitySplitting, scope_query::ScopeQuery, temporal_query},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Query Temporal Evolution of Entities by type with `splitEntities` true and `scopeQ` /Barcelona over a local
/// Entity scoped /Madrid and a source answering Entities scoped /Barcelona and /Madrid answers only the remote one
/// scoped /Barcelona, and the forwarded query carries no `scopeQ`
/// Clause: 5.7.4.4, 4.19, 6.4.3.2
#[gtest]
#[tokio::test]
async fn n_fed_tmp_0003_with_split_entities_a_temporal_scope_q_is_applied_after_the_merge_and_not_forwarded() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_2_id = namespace.urn("Vehicle", 2)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id)
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(50).observed_at("2024-01-01T12:00:00Z".parse()?))
        .build()?;
    // The model refuses it (expected a JSON object at scope), so `scope` is written as text.
    let scope: RawJson = r#""/Madrid""#.parse()?;
    let document = UncheckedDocument::try_from(vehicle_evolution.render(Lossless::Normalized))?.set_member("scope", &scope);
    let request = broker
        .unchecked(&CreateOrUpdateTemporalEvolution::builder().entity(&vehicle_evolution).build())?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    // The source answers evolutions whose scope is a plain string, which the model refuses, so the answer is written as text.
    source
        .answer_when(
            SourceRequest::new(Method::GET, "/a/ngsi-ld/v1/temporal/entities").query_containing("type", format!("Vehicle_{namespace}")),
            source_answer::text(
                StatusCode::OK,
                HeaderValue::from_static("application/json"),
                &format!(
                    r#"[{{"id": "urn:ngsi-ld:Vehicle:{namespace}-2", "type": "Vehicle_{namespace}", "speed": [{{"type": "Property", "value": 50, "observedAt": "2024-01-01T12:00:00Z"}}], "scope": "/Barcelona"}}, {{"id": "urn:ngsi-ld:Vehicle:{namespace}-3", "type": "Vehicle_{namespace}", "speed": [{{"type": "Property", "value": 50, "observedAt": "2024-01-01T12:00:00Z"}}], "scope": "/Madrid"}}]"#
                ),
            ),
        )
        .await?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::QueryTemporal])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let page = broker
        .with_accept(Accept::any())
        .temporal()
        .query(temporal_query::before("2030-01-01T00:00:00Z".parse()?))
        .entity_type(&vehicle_type)
        .split_entities(EntitySplitting::Split)
        .scope_q("/Barcelona".parse::<ScopeQuery>()?)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&vehicle_2_id)));
    let forwarded_request = SourceRequest::new(Method::GET, "/a/ngsi-ld/v1/temporal/entities");
    let forwarded = source.received_matching(&forwarded_request).await?;
    expect_that!(forwarded, len(eq(1)));
    expect_that!(query_values(forwarded.first().or_fail()?, "scopeQ"), is_empty());
    Ok(())
}
