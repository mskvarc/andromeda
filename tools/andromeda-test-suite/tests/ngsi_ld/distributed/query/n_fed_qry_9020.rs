//! A scope query reaches the matching scoped and the unscoped registrations only.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    identity_matcher::has_id,
    namespace::Namespace,
    problem::answered,
    source_answer,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::RegisterContextSource},
    model::{Entity, RawJson},
    query::scope_query::ScopeQuery,
    registration::{CSourceRegistration, DistributedOperation, RegistrationInfo},
};

/// Pins: Query Entities by type with `scopeQ` /Madrid over three `queryEntity` registrations of the type, one
/// scoped /Madrid, one scoped /Barcelona and one declaring no scope, is forwarded once to the /Madrid source and
/// once to the unscoped source and never to the /Barcelona source
/// Clause: 5.7.2.4, 5.12, 5.2.9
#[gtest]
#[tokio::test]
async fn n_fed_qry_9020_a_scope_query_reaches_the_matching_scoped_and_the_unscoped_registrations_only() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let vehicle_madrid_id = namespace.urn("Vehicle", "madrid")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_barcelona_id = namespace.urn("Vehicle", "barcelona")?;
    let vehicle_anywhere_id = namespace.urn("Vehicle", "anywhere")?;
    let registration_madrid_id = namespace.registration_id("madrid")?;
    let registration_barcelona_id = namespace.registration_id("barcelona")?;
    let registration_anywhere_id = namespace.registration_id("anywhere")?;
    let source_entities = [Entity::output_builder()
        .id(vehicle_madrid_id.clone())
        .entity_type(vehicle_type.clone())
        .scope("/Madrid")
        .build()?];
    source
        .answer(Method::GET, "/madrid/ngsi-ld/v1/entities", source_answer::entities(&source_entities))
        .await?;
    let source_entities_2 = [Entity::output_builder()
        .id(vehicle_barcelona_id.clone())
        .entity_type(vehicle_type.clone())
        .scope("/Barcelona")
        .build()?];
    source
        .answer(Method::GET, "/barcelona/ngsi-ld/v1/entities", source_answer::entities(&source_entities_2))
        .await?;
    let source_entities_3 = [Entity::output_builder()
        .id(vehicle_anywhere_id.clone())
        .entity_type(vehicle_type.clone())
        .scope("/Madrid")
        .build()?];
    source
        .answer(Method::GET, "/anywhere/ngsi-ld/v1/entities", source_answer::entities(&source_entities_3))
        .await?;
    let registration_madrid = CSourceRegistration::builder(source.endpoint("madrid")?)
        .id(registration_madrid_id.clone())
        .information([RegistrationInfo::builder().entities([vehicle_type.as_str()]).build()?])
        .operations([DistributedOperation::QueryEntity])
        .build()?;
    // The model refuses it (a registration scope), so `scope` is written as text.
    let scope: RawJson = r#"["/Madrid"]"#.parse()?;
    let document = UncheckedDocument::try_from(registration_madrid.render())?.set_member("scope", &scope);
    let request = broker
        .unchecked(&RegisterContextSource {
            registration: &registration_madrid,
        })?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_madrid_cleanup = Cleanup::of(&broker).registration(&registration_madrid_id);
    let registration_barcelona = CSourceRegistration::builder(source.endpoint("barcelona")?)
        .id(registration_barcelona_id.clone())
        .information([RegistrationInfo::builder().entities([vehicle_type.as_str()]).build()?])
        .operations([DistributedOperation::QueryEntity])
        .build()?;
    // The model refuses it (a registration scope), so `scope` is written as text.
    let scope_2: RawJson = r#"["/Barcelona"]"#.parse()?;
    let document_2 = UncheckedDocument::try_from(registration_barcelona.render())?.set_member("scope", &scope_2);
    let request_2 = broker
        .unchecked(&RegisterContextSource {
            registration: &registration_barcelona,
        })?
        .document(&document_2);
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, answered(StatusCode::CREATED));
    let _registration_barcelona_cleanup = Cleanup::of(&broker).registration(&registration_barcelona_id);
    let registration_anywhere = CSourceRegistration::builder(source.endpoint("anywhere")?)
        .id(registration_anywhere_id.clone())
        .information([RegistrationInfo::builder().entities([vehicle_type.as_str()]).build()?])
        .operations([DistributedOperation::QueryEntity])
        .build()?;
    broker.registrations().register(&registration_anywhere).send().await?;
    let _registration_anywhere_cleanup = Cleanup::of(&broker).registration(&registration_anywhere_id);
    let page = broker
        .with_accept(Accept::any())
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .scope_q("/Madrid".parse::<ScopeQuery>()?)
        .send()
        .await?;
    expect_that!(page.items(), contains(has_id(&vehicle_madrid_id)));
    expect_that!(page.items(), contains(has_id(&vehicle_anywhere_id)));
    expect_that!(page.items(), not(contains(has_id(&vehicle_barcelona_id))));
    let forwarded = source.received_on(Method::GET, "/madrid/ngsi-ld/v1/entities").await?;
    expect_that!(forwarded, len(eq(1)));
    // Table 5.2.9-1: scope lists the Scopes for which the Context Source has
    // Entities, so a registration declaring none may hold an Entity in /Madrid.
    let forwarded_2 = source.received_on(Method::GET, "/anywhere/ngsi-ld/v1/entities").await?;
    expect_that!(forwarded_2, len(eq(1)));
    let forwarded_3 = source.received_on(Method::GET, "/barcelona/ngsi-ld/v1/entities").await?;
    expect_that!(forwarded_3, len(eq(0)));
    Ok(())
}
