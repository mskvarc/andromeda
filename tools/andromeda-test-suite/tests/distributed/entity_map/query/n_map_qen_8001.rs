//! A map over four domains counts every entity, and scoped queries sent at once are all answered.

use andromeda_test_suite::{
    answered_status::AnsweredStatus,
    broker,
    cleanup::Cleanup,
    hub_domains::{DOMAINS, Domain},
    namespace::Namespace,
    outcome::Done,
    received_status::ExpectedStatus,
};
use futures::future::join_all;
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        headers::Accept,
        operation::{EntityMapRequest, ResultsCount},
    },
    model::{Entity, EntityTypeName, Input, NgsiLdMediaType, OneOrMany, Scope},
    query::scope_query,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// The Entities each domain holds.
const ENTITIES_PER_DOMAIN: u32 = 1000;

/// The Entities the four domains hold together.
const ENTITIES: u64 = 4000;

/// The `limit` of the query that creates the map.
const FIRST_PAGE: u32 = 250;

/// The scoped queries sent at once, sixteen per domain.
const CONCURRENT_QUERIES: usize = 64;

/// The scope every Entity `domain` holds carries.
fn domain_scope(domain: &Domain) -> String {
    format!("/Europe/{}", domain.name)
}

/// The Entities `domain` holds, each of `building_type` in the scope of the domain.
fn domain_buildings(namespace: &Namespace, building_type: &EntityTypeName, domain: &Domain) -> Result<Vec<Entity<Input>>> {
    let scope = domain_scope(domain);
    (0..ENTITIES_PER_DOMAIN)
        .map(|number| -> Result<Entity<Input>> {
            Ok(Entity::builder()
                .id(namespace.urn("Building", format!("{}-{number}", domain.name))?)
                .entity_type(building_type.clone())
                .property("speed", number)
                .scope(scope.as_str())
                .build()?)
        })
        .collect()
}

/// Pins: Query Entities on broker A by type with `entityMap` true, limit 250 and count true, over inclusive
/// registrations of brokers B to E holding 1,000 Entities each in a scope of their own, answers 201 with an
/// `EntityMap`, at most 250 Entities and a count of 4,000; 64 Query Entities by type and `scopeQ` sent at once, each
/// naming one domain's scope and its descendants, are each answered 200 with Entities of that scope only
/// Clause: 5.7.2.4, 5.5.14, 5.5.9, 4.19
#[gtest]
#[tokio::test]
async fn n_map_qen_8001_a_map_over_four_domains_counts_every_entity_and_scoped_queries_sent_at_once_are_all_answered() -> Result<()> {
    let namespace = Namespace::fresh();
    let hub = broker::front()?;
    let building_type = namespace.entity_type("Building")?;
    let mut registrations_cleanup = Cleanup::of(&hub);
    for domain in &DOMAINS {
        let registration = CSourceRegistration::builder(broker::peer(domain.peer)?)
            .id(namespace.registration_id(domain.name)?)
            .information([RegistrationInfo::builder()
                .entities([EntityInfo::new(building_type.clone()).with_id_pattern(namespace.id_pattern("Building")?)])
                .build()?])
            .mode(RegistrationMode::Inclusive)
            .operations([DistributedOperation::QueryEntity])
            .build()?;
        hub.registrations().register(&registration).send().await?;
        // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
        // cooldown that opens there holds back a later test's registration naming the same endpoint.
        registrations_cleanup = registrations_cleanup.registration_of(&registration);
        let buildings = domain_buildings(&namespace, &building_type, domain)?;
        broker::client(domain.broker)?.entities().batch_create(&buildings).send().await?.done()?;
    }
    let _registrations_cleanup = registrations_cleanup;
    let entity_map_request = EntityMapRequest::returned();
    let first = hub
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .entities()
        .query()
        .entity_type(&building_type)
        .entity_map(&entity_map_request)
        .limit(FIRST_PAGE)
        .count(ResultsCount::Include)
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    expect_that!(first.total(), some(eq(ENTITIES)));
    expect_that!(first.items(), all!(not(is_empty()), len(le(usize::try_from(FIRST_PAGE)?))));
    expect_that!(first.entity_map().valid(), some(anything()));
    let scoped = hub.with_heads().with_accept(Accept::new(NgsiLdMediaType::JsonLd));
    let queried = DOMAINS.iter().cycle().take(CONCURRENT_QUERIES).collect::<Vec<_>>();
    let answers = join_all(queried.iter().map(|domain| async {
        scoped
            .entities()
            .query()
            .entity_type(&building_type)
            .scope_q(scope_query::subtree(domain_scope(domain)))
            .send()
            .await
    }))
    .await;
    expect_that!(
        answers.iter().map(AnsweredStatus::answered_status).collect::<Vec<_>>(),
        each(some(eq(&StatusCode::OK)))
    );
    for (domain, answer) in queried.iter().zip(&answers) {
        let expected_scope = OneOrMany::one(domain_scope(domain).parse::<Scope>()?);
        if let Ok(answered) = answer {
            expect_that!(
                answered.outcome.items(),
                all!(not(is_empty()), each(field!(Entity.scope, some(eq(&expected_scope)))))
            );
        }
    }
    Ok(())
}
