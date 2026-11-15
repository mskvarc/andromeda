//! A source failing the probe for the Scope in force adds a warning.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    header_matcher::present,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    problem::answered,
    source_answer,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::CreateOrUpdateTemporalEvolution},
    model::{ErrorType, Lossless, Property, RawJson, evolution::TemporalEntity},
    query::{EntitySplitting, scope_query::ScopeQuery, temporal_query},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo},
};

/// Pins: under `splitEntities` a between 12:00-13:00 `scopeQ=/Madrid` query answers 200 with an NGSILD-Warning when
/// the source answers the window query and fails the probe for the Scope in force when the window opens with 500,
/// the Vehicle still judged on what the other parts hold
/// Clause: 5.7.4.4, 6.3.17, 4.18
#[gtest]
#[tokio::test]
async fn o_fed_tmp_7449_a_source_failing_the_probe_for_the_scope_in_force_adds_a_warning() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(60).observed_at("2020-08-01T12:10:00Z".parse()?))
        .build()?;
    // The model refuses it (a temporal scope), so `scope` is written as text.
    let scope: RawJson = r#"[{"type": "Property", "value": "/Madrid", "observedAt": "2020-08-01T11:00:00Z"}]"#.parse()?;
    let document = UncheckedDocument::try_from(vehicle_evolution.render(Lossless::Normalized))?.set_member("scope", &scope);
    let request = broker
        .unchecked(&CreateOrUpdateTemporalEvolution::builder().entity(&vehicle_evolution).build())?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .operations([DistributedOperation::QueryTemporal])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let source_problem = source_answer::problem_without_detail(ErrorType::InternalError, "failure", None);
    source
        .answer_when(
            SourceRequest::new(Method::GET, "/a/ngsi-ld/v1/temporal/entities")
                .query("timerel", "before")
                .priority(1),
            source_answer::problem(StatusCode::INTERNAL_SERVER_ERROR, &source_problem),
        )
        .await?;
    let source_evolutions = [];
    source
        .answer_when(
            SourceRequest::new(Method::GET, "/a/ngsi-ld/v1/temporal/entities").priority(10),
            source_answer::temporal_entities(&source_evolutions),
        )
        .await?;
    let page = broker
        .with_accept(Accept::any())
        .temporal()
        .query(temporal_query::between("2020-08-01T12:00:00Z".parse()?, "2020-08-01T13:00:00Z".parse()?))
        .entity_type(&vehicle_type)
        .split_entities(EntitySplitting::Split)
        .scope_q("/Madrid".parse::<ScopeQuery>()?)
        .send()
        .await?;
    expect_that!(page.warnings(), present());
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&vehicle_id)));
    Ok(())
}
