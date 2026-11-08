//! Last n queries sent at once over the histories of four domains are all answered with the latest instances.

use andromeda_test_suite::{
    answered_status::AnsweredStatus,
    attribute_matcher::any_attribute,
    broker,
    cleanup::Cleanup,
    evolution_matcher::has_evolution,
    header_matcher::absent,
    hub_domains::{DOMAINS, Domain},
    namespace::Namespace,
    outcome::Upserted,
};
use futures::future::join_all;
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{EntityTypeName, Input, Property, evolution::TemporalEntity},
    query::temporal_query,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationManagementInfo, TimeInterval},
};

/// The Entities whose history each domain holds.
const EVOLUTIONS_PER_DOMAIN: u32 = 250;

/// The `speed` instances of one history, one a day from 2024-01-01.
const INSTANCES_PER_EVOLUTION: u32 = 20;

/// The `lastN` of every query.
const LAST_N: u64 = 5;

/// The temporal queries sent at once.
const CONCURRENT_QUERIES: usize = 64;

/// The registrations' forwarding timeout in milliseconds, which takes precedence over the broker's 5 s default. Each
/// domain answers its share of the [`CONCURRENT_QUERIES`] at once in about a second when its host is otherwise idle,
/// and the tree's other tests share that host; the bound stays below the hub's own 30 s request timeout, so a domain
/// that misses it is still reported in the answer, as a 199 `NGSILD-Warning` the test fails on.
const FORWARDING_TIMEOUT_MS: u32 = 20_000;

/// The history of the `number`-th Entity of `domain`: `speed` 0 to 19, observed at midnight of 1 to 20 January 2024.
fn building_evolution(namespace: &Namespace, building_type: &EntityTypeName, domain: &Domain, number: u32) -> Result<TemporalEntity<Input>> {
    let evolution = TemporalEntity::builder()
        .id(namespace.urn("Building", format!("{}-{number}", domain.name))?)
        .entity_type(building_type.clone());
    let evolution = (0..INSTANCES_PER_EVOLUTION).try_fold(evolution, |evolution, day| -> Result<_> {
        let observed_at = format!("2024-01-{:02}T00:00:00Z", day + 1).parse()?;
        Ok(evolution.property("speed", Property::builder(day).observed_at(observed_at)))
    })?;
    Ok(evolution.build()?)
}

/// Makes `domain` hold the histories of its Entities, one Create or Update Temporal Evolution of an Entity each.
async fn hold_evolutions(namespace: &Namespace, building_type: &EntityTypeName, domain: &Domain) -> Result<()> {
    let holder = broker::client(domain.broker)?;
    for number in 0..EVOLUTIONS_PER_DOMAIN {
        let evolution = building_evolution(namespace, building_type, domain, number)?;
        holder.temporal().create_or_update(&evolution).send().await?.created()?;
    }
    Ok(())
}

/// Pins: 64 Query Temporal Evolution of Entities on broker A by type, between 2024-01-01 and 2024-02-01 with `lastN`
/// 5, sent at once over `queryTemporal` registrations of brokers B to E with a 20 s `management.timeout`, holding 250
/// histories of 20 `speed` instances each, are each answered 200 without an `NGSILD-Warning`, every domain having
/// answered, with histories that each keep exactly the five latest instances
/// Clause: 5.7.4.4
#[gtest]
#[tokio::test]
async fn n_fed_tmp_7435_last_n_queries_sent_at_once_over_the_histories_of_four_domains_are_all_answered_with_the_latest_instances() -> Result<()> {
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
            .operations([DistributedOperation::QueryTemporal])
            .observation_interval(TimeInterval::between("2024-01-01T00:00:00Z".parse()?, "2025-01-01T00:00:00Z".parse()?))
            .management(RegistrationManagementInfo::builder().timeout(FORWARDING_TIMEOUT_MS).build()?)
            .build()?;
        hub.registrations().register(&registration).send().await?;
        // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
        // cooldown that opens there holds back a later test's registration naming the same endpoint.
        registrations_cleanup = registrations_cleanup.registration_of(&registration);
    }
    let _registrations_cleanup = registrations_cleanup;
    join_all(DOMAINS.iter().map(|domain| hold_evolutions(&namespace, &building_type, domain)))
        .await
        .into_iter()
        .collect::<Result<Vec<()>>>()?;
    let window_start = "2024-01-01T00:00:00Z".parse()?;
    let window_end = "2024-02-01T00:00:00Z".parse()?;
    let querying = hub.with_heads().with_accept(Accept::any());
    let answers = join_all((0..CONCURRENT_QUERIES).map(|_| async {
        querying
            .temporal()
            .query(temporal_query::between(window_start, window_end).last_n(LAST_N))
            .entity_type(&building_type)
            .send()
            .await
    }))
    .await;
    expect_that!(
        answers.iter().map(AnsweredStatus::answered_status).collect::<Vec<_>>(),
        each(some(eq(&StatusCode::OK)))
    );
    for answered in answers.iter().flatten() {
        // A domain that misses the timeout is left out of the answer and named in a 199 warning (clause 6.3.17).
        expect_that!(answered.outcome.warnings(), absent());
        // lastN keeps the latest instances: those of 16 to 20 January, `speed` 15 to 19.
        expect_that!(
            answered.outcome.items(),
            all!(
                not(is_empty()),
                each(has_evolution(
                    "speed",
                    all!(
                        len(eq(5)),
                        contains(any_attribute().value(15)),
                        contains(any_attribute().value(16)),
                        contains(any_attribute().value(17)),
                        contains(any_attribute().value(18)),
                        contains(any_attribute().value(19))
                    )
                ))
            )
        );
    }
    Ok(())
}
