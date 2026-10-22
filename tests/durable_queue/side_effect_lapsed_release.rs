//! A replay that fails after outliving its lease still releases its job for a
//! paced retry.
//!
//! A replay that began under its lease keeps the job to itself until its
//! transaction ends, however long past that lease it runs: no other worker can
//! have claimed the job meanwhile. The worker is therefore still the job's
//! claimant when such a replay fails, and its release must land -- recording
//! the failure and scheduling the retry one backoff after the release -- rather
//! than be refused because the lease's wall-clock end has passed. A refused
//! release would leave the job claimable at once, with no failure recorded, for
//! the next claim to count as a lapsed attempt; a retry stamped from the start
//! of the pass would fall due one overrun early.
//!
//! The replay is held on the temporal history relations until well past its
//! one-second lease, and then failed ([`crate::replay_interruption`]).

use crate::{
    live_side_effect_drain::live_side_effect_drain,
    queue_database::{queue_database, side_effect_queue},
    queue_workers::side_effect_worker,
    replay_interruption::{enqueue_observation, pass_with_failed_replay},
};
use andromeda_entity::{side_effect_drain::EntitySideEffectDrainConfig, side_effect_queue::SideEffectQueue};
use andromeda_postgresql_schema::queue::PendingSideEffects;
use andromeda_postgresql_seaquery::{catalog_function::num_nonnulls, statement::Statement};
use chrono::{DateTime, TimeDelta, Utc};
use pretty_assertions::assert_eq;
use sea_query::{Expr, ExprTrait, Query};
use serial_test::serial;
use std::time::Duration;

/// The Entity whose observation the job still owes its Temporal Evolution.
const ENTITY_ID: &str = "urn:ngsi-ld:Vehicle:lapsed-release";
/// The drain's lease: the shortest one the verification can wait out.
const LEASE_SECONDS: i32 = 1;
/// How long past its lease the replay is held before it fails.
const LEASE_OVERRUN: Duration = Duration::from_millis(1_500);
/// The drain's base retry backoff: long enough that a retry scheduled at it is
/// unmistakably in the future.
const BASE_BACKOFF: Duration = Duration::from_secs(60);

#[tokio::test]
#[serial(durable_queue)]
async fn durable_queue_a_replay_that_fails_after_outliving_its_lease_releases_its_job_for_a_paced_retry() {
    let pool = queue_database().await;
    let write_id = enqueue_observation(&pool, ENTITY_ID).await;
    let (drain, _published) = live_side_effect_drain(
        &pool,
        EntitySideEffectDrainConfig {
            lease_seconds: LEASE_SECONDS,
            // One job per pass, so a job released for an immediate reclaim is
            // not replayed again by the same pass.
            batch_size: 1,
            max_attempts: 5,
            base_backoff: BASE_BACKOFF,
            lease_holder: side_effect_worker(),
        },
    );
    let (outcome, failed_at) = pass_with_failed_replay(&pool, &drain, LEASE_OVERRUN).await;
    assert_eq!(outcome.completed, 0, "the failed replay settled nothing");
    let (claim_columns, attempts, failure_recorded, next_attempt_at, exhausted): (i32, i32, bool, DateTime<Utc>, bool) = Query::select()
        .expr(num_nonnulls([
            Expr::col(PendingSideEffects::ClaimedAt),
            Expr::col(PendingSideEffects::ClaimedBy),
            Expr::col(PendingSideEffects::LeaseUntil),
        ]))
        .column(PendingSideEffects::AttemptCount)
        .expr(Expr::col(PendingSideEffects::LastError).is_not_null())
        .column(PendingSideEffects::NextAttemptAt)
        .expr(Expr::col(PendingSideEffects::ExhaustedAt).is_not_null())
        .from(PendingSideEffects::Table)
        .and_where(Expr::col(PendingSideEffects::WriteId).eq(write_id.as_uuid()))
        .to_query_as()
        .fetch_one(&pool)
        .await
        .expect("the released job");
    let backoff = TimeDelta::from_std(BASE_BACKOFF).expect("the backoff fits");
    assert!(failure_recorded, "the release records why the replay failed");
    assert!(
        next_attempt_at >= failed_at + backoff,
        "the retry falls due one backoff after the release, not after the start of the pass: {next_attempt_at} precedes {}",
        failed_at + backoff
    );
    assert_eq!(claim_columns, 0, "the release lets the lease go");
    assert_eq!(attempts, 1, "the failed attempt is counted once");
    assert!(!exhausted, "one failure does not exhaust the job");
    let contended = side_effect_queue(&pool)
        .lease_batch(&side_effect_worker(), 1, LEASE_SECONDS)
        .await
        .expect("claim after the release");
    assert!(
        contended.is_empty(),
        "the released job waits for its retry instead of being claimable at once: {contended:?}"
    );
    pool.close().await;
}
