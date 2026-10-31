//! An unscoped source receives the scope query and its answer stays.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    identity_matcher::has_id,
    namespace::Namespace,
    source_answer,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::Method;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, OneOrMany, Scope},
    query::{EntitySplitting, scope_query::ScopeQuery},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo},
};

/// Pins: Query Entities by type with `scopeQ` /Madrid and `splitEntities` false over one `queryEntity` registration
/// of the type declaring no scope is forwarded once with `scopeQ` /Madrid, and the Entity the source returns is in
/// the answer
/// Clause: 5.7.2.4, 5.12, 4.3.6.7
#[gtest]
#[tokio::test]
async fn n_fed_qry_9398_an_unscoped_source_receives_the_scope_query_and_its_answer_stays() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let registration_id = namespace.registration_id(1)?;
    let madrid: Scope = "/Madrid".parse()?;
    let source_entities = [Entity::output_builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .scope("/Madrid")
        .build()?];
    source
        .answer(Method::GET, "/a/ngsi-ld/v1/entities", source_answer::entities(&source_entities))
        .await?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .operations([DistributedOperation::QueryEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // A forward to a registration left behind fails at its endpoint, and the cooldown that opens there holds back a
    // later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let page = broker
        .with_accept(Accept::any())
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .scope_q("/Madrid".parse::<ScopeQuery>()?)
        .split_entities(EntitySplitting::Whole)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), contains(has_id(&vehicle_id)));
    let answered = page.items().iter().find(|item| item.id == vehicle_id).or_fail()?;
    expect_that!(answered.scope.as_ref().map(OneOrMany::as_slice), some(elements_are![eq(&madrid)]));
    let forwarded = source.received_on(Method::GET, "/a/ngsi-ld/v1/entities").await?;
    expect_that!(forwarded, len(eq(1)));
    // Clause 4.3.6.7: without split entities the source applies the scope query itself.
    let forwarded_with_scope = source
        .received_matching(&SourceRequest::new(Method::GET, "/a/ngsi-ld/v1/entities").query("scopeQ", "/Madrid"))
        .await?;
    expect_that!(forwarded_with_scope, len(eq(1)));
    Ok(())
}
