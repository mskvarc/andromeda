//! A source entity with nothing in the window does not appear.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    evolution_matcher::has_evolution,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
    problem::problem,
    source_answer,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::temporal_query,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo},
};

/// Pins: Under an inclusive source holding Vehicle -2 alone, whose speed it answers observed on 2020-07-31 only,
/// Query Temporal Evolution of Entities after 2020-08-01 answers the local Vehicle -1 without -2, and after
/// 2020-07-01 answers both; Retrieve Temporal Evolution of -2 after 2020-08-01 answers 404 `ResourceNotFound`, and
/// without a temporal query answers 200 with the instance
/// Clause: 4.11, 5.7.3.4, 5.7.4.4
#[gtest]
#[tokio::test]
async fn n_fed_tmp_9019_a_source_entity_with_nothing_in_the_window_does_not_appear() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_2_id = namespace.urn("Vehicle", 2)?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(120).observed_at("2020-08-01T12:03:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_2_id.clone())])
            .build()?])
        .operations([
            DistributedOperation::RetrieveEntity,
            DistributedOperation::QueryEntity,
            DistributedOperation::RetrieveTemporal,
            DistributedOperation::QueryTemporal,
        ])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let source_evolutions = [TemporalEntity::builder()
        .id(vehicle_2_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(1).observed_at("2020-07-31T12:00:00Z".parse()?))
        .build()?];
    source
        .answer_when(
            SourceRequest::new(Method::GET, "/a/ngsi-ld/v1/temporal/entities").query_containing("type", format!("Vehicle_{namespace}")),
            source_answer::temporal_entities(&source_evolutions),
        )
        .await?;
    let source_evolution = TemporalEntity::builder()
        .id(vehicle_2_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(1).observed_at("2020-07-31T12:00:00Z".parse()?))
        .build()?;
    source
        .answer(
            Method::GET,
            &format!("/a/ngsi-ld/v1/temporal/entities/{vehicle_2_id}"),
            source_answer::temporal_entity(&source_evolution),
        )
        .await?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2020-08-01T00:00:00Z".parse()?))
        .entity_type(&vehicle_type)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&vehicle_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2020-07-01T00:00:00Z".parse()?))
        .entity_type(&vehicle_type)
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(2)));
    expect_that!(
        page_2.items(),
        contains(all!(has_id(&vehicle_2_id), has_evolution("speed", nth(0, any_attribute().value(1)))))
    );
    let answer = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_2_id)
        .temporal_q(temporal_query::after("2020-08-01T00:00:00Z".parse()?))
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_2_id)
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(evolution.id, eq(&vehicle_2_id));
    expect_that!(evolution, has_evolution("speed", all!(len(eq(1)), nth(0, any_attribute().value(1)))));
    Ok(())
}
