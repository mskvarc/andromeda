//! A side-effect job is settled by the replay commit that lands its
//! consequences, in that commit's own transaction and under the replaying
//! worker's lease.
//!
//! A worker whose lease passed to another before its replay began must leave
//! nothing behind -- neither the replay's consequences nor a missing job -- so
//! that the worker now holding the job replays it and its consequences land
//! exactly once. Had the replay and the job's removal been two transactions, a
//! lease lost between them would let the new holder replay a job whose
//! notifications had already been queued, and a live-delivery broker would
//! then notify them a second time.
//!
//! A replay that did begin under its lease keeps the job to itself until it
//! commits, however long it runs past that lease: no other worker can claim
//! the job in the meantime, so its consequences still land once.

use crate::{
    broker_committer::committer,
    queue_database::{deliveries_of, enqueue_side_effect, events_of, live_target, queue_database, side_effect_queue, subscription, subscription_key},
    queue_workers::side_effect_worker,
};
use andromeda_context_commit::{
    atomic_write::AtomicWrite,
    commit_replay::CommitReplay,
    committer::ContextCommitter,
    entity_mutation::EntityMutation,
    error::CommitError,
    job_lease::JobLease,
    lease_holder::LeaseHolder,
    subscription_match::SubscriptionMatch,
};
use andromeda_entity::side_effect_queue::{LeasedSideEffect, SideEffectQueue};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::entity_change::{EntityChange, EntityLifecycle};
use pretty_assertions::assert_eq;
use serial_test::serial;
use sqlx::PgPool;
use std::{sync::Arc, time::Duration};
use tokio::time::{Instant, sleep};

/// Jobs leased per pass, larger than the one fixture here.
const BATCH_SIZE: i32 = 10;
/// The shortest lease the verification can wait out.
const LEASE_SECONDS: i32 = 1;
/// The entity the replayed notification is about.
const ENTITY_ID: &str = "urn:ngsi-ld:Vehicle:replay-settlement";
/// How long past its lease the slow replay is held.
const LEASE_OVERRUN: Duration = Duration::from_millis(1_500);
/// The longest the slow replay is waited for to reach the lock that holds it.
const BLOCKED_DEADLINE: Duration = Duration::from_secs(10);

/// The one write a replay of the job submits: a notification owed to the
/// seeded subscription, leaving the entity row alone.
fn owed_notification(stored_subscription: i64) -> Vec<AtomicWrite> {
    let entity_id: Identifier = ENTITY_ID.parse().expect("a valid entity URI");
    vec![
        AtomicWrite::builder()
            .entity_id(Arc::new(entity_id.clone())) // clone: the write and the match each own the identifier.
            .entity(EntityMutation::None)
            .subscription_matches(vec![
                SubscriptionMatch::builder()
                    .subscription_key(subscription_key(stored_subscription))
                    .entity_id(entity_id)
                    .change(EntityChange::new(EntityLifecycle::Created, Vec::new()))
                    .build(),
            ])
            .build(),
    ]
}

/// The replay continuation of a leased job under `holder`'s lease.
const fn replay_of(leased: &LeasedSideEffect, holder: LeaseHolder) -> CommitReplay {
    CommitReplay::new(leased.record, leased.base_ordinal, JobLease { job: leased.id, holder })
}

/// Who holds the job's lease now, if the job is still queued.
async fn job_holder(pool: &PgPool, leased: &LeasedSideEffect) -> Option<uuid::Uuid> {
    sqlx::query_scalar("SELECT claimed_by FROM pending_side_effects WHERE id = $1")
        .bind(leased.id.as_uuid())
        .fetch_optional(pool)
        .await
        .expect("job state")
        .flatten()
}

#[tokio::test]
#[serial(durable_queue)]
async fn durable_queue_a_replay_under_a_reclaimed_lease_commits_nothing_and_the_new_holders_replay_lands_once() {
    let pool = queue_database().await;
    let stored_subscription = subscription(&pool, "urn:ngsi-ld:Subscription:replay-settlement").await;
    enqueue_side_effect(&pool).await;
    let repository = side_effect_queue(&pool);
    let committer = committer(&pool);
    let first = side_effect_worker();
    let second = side_effect_worker();

    let leased = repository.lease_batch(&first, BATCH_SIZE, LEASE_SECONDS).await.expect("first lease");
    let leased = leased.into_iter().next().expect("the due job is leased to the first worker");
    // The first worker stalls past its lease, and the second reclaims the job.
    sleep(Duration::from_millis(1_500)).await;
    let reclaimed = repository.lease_batch(&second, BATCH_SIZE, LEASE_SECONDS * 30).await.expect("reclaim");
    assert_eq!(reclaimed.len(), 1, "the expired lease makes the job claimable again");

    let stale = committer
        .commit_replayed(&live_target(), replay_of(&leased, first), owed_notification(stored_subscription))
        .await;
    assert!(
        matches!(stale, Err(CommitError::ReplayLeaseLost { job }) if job == leased.id),
        "a replay whose lease passed on is refused: {stale:?}"
    );
    assert_eq!(events_of(&pool, &leased.record).await, 0, "the refused replay left no notification event");
    assert_eq!(deliveries_of(&pool, stored_subscription).await, 0, "the refused replay queued no delivery");
    assert_eq!(
        job_holder(&pool, &leased).await,
        Some(second.as_uuid()),
        "the job stays queued under the worker that reclaimed it"
    );

    committer
        .commit_replayed(&live_target(), replay_of(&leased, second), owed_notification(stored_subscription))
        .await
        .expect("the current holder's replay lands");
    assert_eq!(events_of(&pool, &leased.record).await, 1, "the current holder's replay records the event once");
    assert_eq!(deliveries_of(&pool, stored_subscription).await, 1, "and queues its delivery once");
    let remaining: i64 = sqlx::query_scalar("SELECT count(*) FROM pending_side_effects WHERE id = $1")
        .bind(leased.id.as_uuid())
        .fetch_one(&pool)
        .await
        .expect("job count");
    assert_eq!(remaining, 0, "the landed replay removed the job in its own transaction");
    pool.close().await;
}

/// Whether another session of the suite's database is waiting for a lock.
async fn a_session_waits_for_a_lock(pool: &PgPool) -> bool {
    sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM pg_stat_activity WHERE datname = current_database() AND wait_event_type = 'Lock')")
        .fetch_one(pool)
        .await
        .expect("session activity")
}

/// A replay holds the job it settles for as long as it runs: once it has
/// begun under its lease, a claim made after that lease lapsed finds nothing
/// to take, and the replay still lands its consequences exactly once.
///
/// The replay is held mid-transaction by a lock on the notification events it
/// has to write, until well past its one-second lease. A replay that settled
/// its job only at the end of its transaction would leave the lapsed job
/// claimable meanwhile, and the claim would hand it to a second worker to
/// replay again.
#[tokio::test]
#[serial(durable_queue)]
async fn durable_queue_a_replay_slower_than_its_lease_keeps_its_job_and_lands_once() {
    let pool = queue_database().await;
    let stored_subscription = subscription(&pool, "urn:ngsi-ld:Subscription:slow-replay").await;
    enqueue_side_effect(&pool).await;
    let repository = side_effect_queue(&pool);
    let first = side_effect_worker();
    let leased = repository.lease_batch(&first, BATCH_SIZE, LEASE_SECONDS).await.expect("first lease");
    let leased = leased.into_iter().next().expect("the due job is leased to the first worker");

    let mut holder = pool.begin().await.expect("lock holder transaction");
    sqlx::query("LOCK TABLE notification_events IN SHARE MODE")
        .execute(&mut *holder)
        .await
        .expect("hold the notification events");
    let replaying = committer(&pool);
    let replay = replay_of(&leased, first);
    let slow_replay = tokio::spawn(async move { replaying.commit_replayed(&live_target(), replay, owed_notification(stored_subscription)).await });
    let deadline = Instant::now() + BLOCKED_DEADLINE;
    while !a_session_waits_for_a_lock(&pool).await {
        assert!(Instant::now() < deadline, "the replay reaches the held lock before the deadline");
        sleep(Duration::from_millis(20)).await;
    }
    sleep(LEASE_OVERRUN).await;

    let contended = repository
        .lease_batch(&side_effect_worker(), BATCH_SIZE, LEASE_SECONDS * 30)
        .await
        .expect("claim while the replay runs");
    assert!(
        contended.is_empty(),
        "a job whose replay is under way is not claimable, however long past its lease: {contended:?}"
    );

    holder.rollback().await.expect("release the notification events");
    slow_replay.await.expect("the replay task ran to completion").expect("the slow replay lands");
    assert_eq!(events_of(&pool, &leased.record).await, 1, "the slow replay records its event once");
    assert_eq!(deliveries_of(&pool, stored_subscription).await, 1, "and queues its delivery once");
    assert_eq!(job_holder(&pool, &leased).await, None, "the landed replay removed the job");
    let after = repository
        .lease_batch(&side_effect_worker(), BATCH_SIZE, LEASE_SECONDS)
        .await
        .expect("claim after the replay");
    assert!(after.is_empty(), "nothing is left to replay again: {after:?}");
    pool.close().await;
}
