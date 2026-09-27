//! A side-effect job is dead-lettered with exactly the attempts its ceiling
//! permitted, however those attempts ended.
//!
//! A worker that crashes, or whose lease lapses before its replay could lock
//! the job, records nothing; the claim that reclaims the job counts that
//! attempt instead. A job whose permitted attempts all ended that way is
//! dead-lettered by the next drain pass that claims it, which makes no attempt
//! of its own and so counts none. A job whose final permitted replay fails is
//! dead-lettered by that replay's release, which counts the failed attempt.
//! Either way the dead letter records the ceiling, not one attempt more.

use crate::{
    live_side_effect_drain::live_side_effect_drain,
    queue_database::{enqueue_side_effect, queue_database},
    queue_workers::side_effect_worker,
    replay_interruption::{enqueue_observation, pass_with_failed_replay},
};
use andromeda_context_commit::write_id::WriteId;
use andromeda_entity::{side_effect_drain::EntitySideEffectDrainConfig, side_effect_queue::SideEffectQueue};
use andromeda_postgresql_context_store::pending_side_effects::repo::PendingSideEffectsRepository;
use chrono::Utc;
use pretty_assertions::assert_eq;
use serial_test::serial;
use sqlx::PgPool;
use std::time::Duration;
use tokio::time::sleep;

/// The attempts the drain permits a job.
const MAX_ATTEMPTS: u16 = 2;
/// The shortest lease these verifications can wait out.
const LEASE_SECONDS: i32 = 1;
/// How long a claim is left to lapse: past its one-second lease.
const LAPSE: Duration = Duration::from_millis(1_500);
/// The drain's base retry backoff.
const BASE_BACKOFF: Duration = Duration::from_secs(1);
/// How long a released job is waited for to fall due again: past the backoff.
const RETRY_DUE: Duration = Duration::from_millis(1_500);
/// How long a failing replay is held before it fails: well inside its lease.
const HELD: Duration = Duration::from_millis(100);

/// A drain permitting [`MAX_ATTEMPTS`] per job, under a lease no verification
/// outlives.
fn ceiling_configuration() -> EntitySideEffectDrainConfig {
    EntitySideEffectDrainConfig {
        lease_seconds: LEASE_SECONDS * 30,
        batch_size: 1,
        max_attempts: MAX_ATTEMPTS,
        base_backoff: BASE_BACKOFF,
        lease_holder: side_effect_worker(),
    }
}

/// Assert that the job `write_id` queued is dead-lettered with exactly
/// [`MAX_ATTEMPTS`] attempts, no lease, and a recorded reason, and is never
/// handed out again.
async fn assert_dead_lettered_at_the_ceiling(pool: &PgPool, write_id: WriteId) {
    let (dead, attempts, lock_columns, last_error): (bool, i32, i32, Option<String>) = sqlx::query_as(
        "SELECT exhausted_at IS NOT NULL, attempt_count, num_nonnulls(lease_until, claimed_by), last_error FROM pending_side_effects WHERE write_id = $1",
    )
    .bind(write_id.as_uuid())
    .fetch_one(pool)
    .await
    .expect("the dead-lettered job is kept as evidence");
    assert!(dead, "the job is dead-lettered");
    assert_eq!(
        attempts,
        i32::from(MAX_ATTEMPTS),
        "the dead letter records exactly the attempts the ceiling permitted"
    );
    assert_eq!(lock_columns, 0, "dead-lettering releases the lease the drain took");
    assert!(last_error.is_some_and(|error| !error.is_empty()), "the reason is recorded");
    let after = PendingSideEffectsRepository::new(pool)
        .lease_batch(&side_effect_worker(), 1, LEASE_SECONDS)
        .await
        .expect("claim after the dead-lettering");
    assert!(after.is_empty(), "a dead job is never handed out again");
}

/// A job claimed as often as the ceiling permits, each claim lapsing without
/// a word, is dead-lettered by the next drain pass: the pass replays nothing,
/// publishes nothing, and counts no attempt of its own.
#[tokio::test]
#[serial(durable_queue)]
async fn durable_queue_a_job_whose_claims_all_lapsed_is_dead_lettered_by_the_next_drain_pass() {
    let pool = queue_database().await;
    let write_id = enqueue_side_effect(&pool).await;
    let repository = PendingSideEffectsRepository::new(&pool);

    for expected_attempts in 0..i32::from(MAX_ATTEMPTS) {
        let claimed = repository.lease_batch(&side_effect_worker(), 1, LEASE_SECONDS).await.expect("claim the job");
        let claimed = claimed.first().expect("the due job is claimed");
        assert_eq!(
            claimed.attempts, expected_attempts,
            "each reclaim counts the attempt whose lease lapsed before it"
        );
        // The worker holding the claim crashes: nothing settles or releases it.
        sleep(LAPSE).await;
    }

    let (drain, published) = live_side_effect_drain(&pool, ceiling_configuration());
    let outcome = drain.execute(Utc::now()).await.expect("the drain pass runs");
    assert_eq!(outcome.completed, 0, "a job past its ceiling is not replayed");
    assert!(published.take().is_empty(), "and publishes nothing");
    assert_dead_lettered_at_the_ceiling(&pool, write_id).await;
    pool.close().await;
}

/// A job whose every permitted replay fails is dead-lettered by the release of
/// the last one, which counts that failed attempt -- and only that one.
#[tokio::test]
#[serial(durable_queue)]
async fn durable_queue_a_job_whose_final_replay_fails_is_dead_lettered_with_the_ceiling_as_its_attempts() {
    let pool = queue_database().await;
    let write_id = enqueue_observation(&pool, "urn:ngsi-ld:Vehicle:final-replay-fails").await;
    let (drain, published) = live_side_effect_drain(&pool, ceiling_configuration());

    for pass in 1..=MAX_ATTEMPTS {
        let (outcome, _failed_at) = pass_with_failed_replay(&pool, &drain, HELD).await;
        assert_eq!(outcome.completed, 0, "replay {pass} failed and settled nothing");
        if pass < MAX_ATTEMPTS {
            sleep(RETRY_DUE).await;
        }
    }
    assert!(published.take().is_empty(), "no failed replay publishes anything");
    assert_dead_lettered_at_the_ceiling(&pool, write_id).await;
    pool.close().await;
}
