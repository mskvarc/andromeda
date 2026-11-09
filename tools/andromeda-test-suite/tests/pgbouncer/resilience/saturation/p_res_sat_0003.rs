//! An Entity create is answered while registration creates queue on a held change clock.

use andromeda_test_suite::{
    broker::{self, BrokerClient},
    change_clock_hold::ChangeClockHold,
    cleanup::Cleanup,
    config::BrokerRole,
    context_source::ContextSource,
    database_name::DatabaseName,
    entity_registration::entity_registration,
    eventually::{Poll, eventually},
    load_outcome::LoadOutcome,
    namespace::Namespace,
    server_backends::{BackendScope, ServerBackends},
    tenant_management::database_of,
    tenant_set::TenantSet,
};
use futures::{future::join_all, join};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::model::{Entity, Input};
use std::time::Duration;
use tokio::time::Instant;

/// How long the change clock is held.
const HELD_FOR: Duration = Duration::from_secs(8);

/// The registration creates sent while the clock is held, twice the four connections of the tenant's pool
/// (`database.pooler.max_connections_per_tenant`), enough to take every one of them if the writes queued on the clock
/// were not bounded.
const QUEUED_CREATES: usize = 8;

/// How long the test waits for a registration create to reach the held row.
const QUEUE_BOUND: Duration = Duration::from_secs(15);

/// Pins: while one session holds the row lock of a tenant database's context source change clock for 8 s, with 8
/// Register Context Source requests sent at once and one of them waiting on the row, Create Entity in the tenant is
/// answered 201 in less than 4 s; each Register Context Source is answered with a success or 503 naming a delay in
/// `Retry-After`; and Register Context Source after the lock is released is answered 201
/// Clause: 5.6.1, 5.9.2, 5.5.6
/// Tenant: private (the subject is the change clock and the pool of a whole tenant database)
/// Recorded: Andromeda deployment, not v1.9.1. Every registration write draws a revision from one row of the tenant's
/// database and waits on its lock holding a pooled connection, and the broker serves its requests through a
/// transaction-pooling `PgBouncer`, four connections per tenant
#[gtest]
#[tokio::test]
async fn p_res_sat_0003_an_entity_create_is_answered_while_registration_creates_queue_on_a_held_change_clock() -> Result<()> {
    let namespace = Namespace::fresh();
    let resilience = broker::client(BrokerRole::Resilience)?;
    let set = TenantSet::provision(&resilience, &namespace, 1, 1).await?;
    let member = &set.member(0).client;
    let database = database_of(&resilience, &set.member(0).tenant).await?;
    let probe_type = namespace.entity_type("ClockProbe")?;
    let source = ContextSource::start().await?;
    let endpoint = source.endpoint("clock")?;
    let mut queued = Vec::with_capacity(QUEUED_CREATES);
    for index in 0..QUEUED_CREATES {
        queued.push(entity_registration(
            &namespace.registration_id(index)?,
            &probe_type,
            namespace.id_pattern("ClockProbe")?,
            endpoint.as_str(),
        )?);
    }
    let after = entity_registration(
        &namespace.registration_id("after")?,
        &probe_type,
        namespace.id_pattern("ClockProbe")?,
        endpoint.as_str(),
    )?;
    let _registration_cleanup = queued.iter().chain([&after]).fold(Cleanup::of(member), Cleanup::registration_of);
    let written = Entity::builder()
        .id(namespace.urn("Device", "written")?)
        .entity_type(namespace.entity_type("Device")?)
        .build()?;
    let tenant = member.with_responses();
    let hold = ChangeClockHold::seize(BrokerRole::Resilience, &database, HELD_FOR).await?;
    let creates = join_all(
        queued
            .iter()
            .map(|registration| async { LoadOutcome::of(&tenant.registrations().register(registration).send().await) }),
    );
    let (registered, entity) = join!(creates, create_once_queued(member, &database, &written));
    let (created, answered_in) = entity?;
    hold.release().await?;
    let registered_after = LoadOutcome::of(&tenant.registrations().register(&after).send().await);
    println!("entity {created:?} in {answered_in:?}; queued {registered:?}; after {registered_after:?}");
    expect_that!(created.status(), some(eq(StatusCode::CREATED)));
    expect_that!(answered_in, lt(HELD_FOR / 2));
    expect_that!(
        registered,
        each(predicate(|outcome: &LoadOutcome| outcome.is_success() || outcome.is_paced_unavailability()))
    );
    verify_that!(registered_after.status(), some(eq(StatusCode::CREATED)))
}

/// Waits until a session of `database` waits on a lock, then creates `entity` through `member`, answering how the
/// create resolved and how long it took.
async fn create_once_queued(member: &BrokerClient, database: &DatabaseName, entity: &Entity<Input>) -> Result<(LoadOutcome, Duration)> {
    let backends = ServerBackends::of(BrokerRole::Resilience)?;
    let waiters = BackendScope::LockWaiters(database.clone());
    eventually(QUEUE_BOUND, || async {
        match backends.count(&waiters).await {
            Ok(0) => Poll::Pending(0_u64),
            Ok(_) => Poll::Ready(Ok(())),
            Err(error) => Poll::Ready(Err(error)),
        }
    })
    .await??;
    let started = Instant::now();
    let created = LoadOutcome::of(&member.with_responses().entities().create(entity).send().await);
    Ok((created, started.elapsed()))
}
