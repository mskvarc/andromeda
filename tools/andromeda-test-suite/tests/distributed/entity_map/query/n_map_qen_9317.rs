//! A posted map asks a registration only for the entity selector it matches.

use andromeda_test_suite::{broker, context_source::ContextSource, namespace::Namespace, problem::answered, source_answer, source_request::SourceRequest};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::operation::QueryMethod,
    query::{EntitySelector, builders::SelectorTypeBuilder},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Create `EntityMap` for Query Entities posted with the selectors {id A, type Vehicle} and {id B, type Boat}
/// under inclusive registrations advertising `createEntityMapQueryEntity`, one of entity A as a Boat and one of
/// entity A as a Vehicle, answers 201 and asks the second source only
/// Clause: 5.14.4.4, 5.12, 5.2.33, 6.34.3.2
/// Tenant: private (it counts every request its context sources receive, and the unscoped requests of other tests
/// reach every registration of the tenant)
#[gtest]
#[tokio::test]
async fn n_map_qen_9317_a_posted_map_asks_a_registration_only_for_the_entity_selector_it_matches() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::private_tenant(&broker::front()?, &namespace).await?;
    let source = ContextSource::start().await?;
    let vehicle_a_id = namespace.urn("Vehicle", "A")?;
    let boat_type = namespace.entity_type("Boat")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(namespace.registration_id(1)?)
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(boat_type).with_id(vehicle_a_id.clone())])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::CreateEntityMapQueryEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    let registration_2 = CSourceRegistration::builder(source.endpoint("b")?)
        .id(namespace.registration_id(2)?)
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type).with_id(vehicle_a_id)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::CreateEntityMapQueryEntity])
        .build()?;
    broker.registrations().register(&registration_2).send().await?;
    for source_path in ["a", "b"] {
        source
            .answer(
                Method::POST,
                &format!("/{source_path}/ngsi-ld/v1/entityMaps"),
                source_answer::status(StatusCode::NOT_FOUND),
            )
            .await?;
        source
            .answer(
                Method::GET,
                &format!("/{source_path}/ngsi-ld/v1/entityMaps"),
                source_answer::status(StatusCode::NOT_FOUND),
            )
            .await?;
    }
    let answer = broker
        .with_responses()
        .entity_maps()
        .create()
        .selectors([
            EntitySelector::new(SelectorTypeBuilder::from(&namespace.entity_type("Vehicle")?).build()?).with_ids(namespace.urn("Vehicle", "A")?),
            EntitySelector::new(SelectorTypeBuilder::from(&namespace.entity_type("Boat")?).build()?).with_ids(namespace.urn("Vehicle", "B")?),
        ])
        .method(QueryMethod::Post)
        .send()
        .await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let asked_a = source.received_matching(&SourceRequest::any_matching("/a/.*")).await?;
    expect_that!(asked_a, len(eq(0)));
    let asked_b = source.received_matching(&SourceRequest::any_matching("/b/.*")).await?;
    expect_that!(asked_b, len(eq(1)));
    Ok(())
}
