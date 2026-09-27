//! Exclusive ownership of a due side-effect job.
//!
//! Side-effect exclusive lease: two workers run against one due row, the owner
//! crashes, and the clock advances past `lease_until`. Only one initial lease
//! exists, the row is reclaimed after expiry, only the current holder's
//! settlement removes it, and only once, a release by the worker whose lease
//! passed on changes nothing, a payload that cannot be decoded
//! reaches a coherent dead state, and a dead row is reaped once its retention
//! has passed while a live row is not.

use crate::{
    queue_database::{enqueue_side_effect, queue_database, settle_side_effect},
    queue_workers::side_effect_worker,
};
use andromeda_context_commit::{
    job_lease::JobLease,
    lease_acknowledgement::LeaseAcknowledgement,
    side_effect_job_id::SideEffectJobId,
    write_id::WriteId,
    write_ordinal::WriteOrdinal,
    write_record::WriteRecord,
};
use andromeda_entity::side_effect_queue::{DeadLetterAttempt, SideEffectQueue};
use andromeda_postgresql_context_store::pending_side_effects::{payload::PersistedBatchSideEffectJob, repo::PendingSideEffectsRepository};
use chrono::{DateTime, Duration as RetentionDuration, Utc};
use pretty_assertions::assert_eq;
use serde_json::json;
use serial_test::serial;
use sqlx::PgPool;
use std::time::Duration;
use tokio::time::sleep;

/// Jobs leased per pass, larger than any fixture in this module.
const BATCH_SIZE: i32 = 10;

/// The shortest lease these verifications can wait out.
const LEASE_SECONDS: i32 = 1;

/// Identity of the single job the queue holds.
async fn queued_job(pool: &PgPool) -> SideEffectJobId {
    let id: uuid::Uuid = sqlx::query_scalar("SELECT id FROM pending_side_effects")
        .fetch_one(pool)
        .await
        .expect("the queued job identity");
    SideEffectJobId::from_uuid(id)
}

#[tokio::test]
#[serial(durable_queue)]
async fn durable_queue_a_due_side_effect_is_leased_once_and_reclaimed_after_its_lease_expires() {
    let pool = queue_database().await;
    let write_id = enqueue_side_effect(&pool).await;
    let repository = PendingSideEffectsRepository::new(&pool);
    let owner = side_effect_worker();
    let contender = side_effect_worker();

    let leased = repository.lease_batch(&owner, BATCH_SIZE, LEASE_SECONDS).await.expect("first lease");
    assert_eq!(leased.len(), 1, "the due job is leased to exactly one worker");
    let job = leased.first().expect("the leased job").id;
    assert_eq!(leased.first().expect("the leased job").attempts, 0);

    let contended = repository.lease_batch(&contender, BATCH_SIZE, LEASE_SECONDS).await.expect("contending lease");
    assert!(contended.is_empty(), "a live lease excludes every other worker");

    // The owner crashes: nothing acknowledges the job, and the lease runs out.
    sleep(Duration::from_millis(1_500)).await;

    let reclaimed = repository.lease_batch(&contender, BATCH_SIZE, LEASE_SECONDS).await.expect("reclaim");
    assert_eq!(reclaimed.len(), 1, "an expired lease makes the job claimable again");
    assert_eq!(reclaimed.first().expect("the reclaimed job").id, job);
    assert_eq!(
        reclaimed.first().expect("the reclaimed job").attempts,
        1,
        "the reclaim counts the attempt whose lease lapsed"
    );

    assert_eq!(
        settle_side_effect(&pool, JobLease { job, holder: owner }).await,
        LeaseAcknowledgement::Lost,
        "the crashed owner's lease passed to the contender, so it can no longer settle the job"
    );
    assert_eq!(
        settle_side_effect(&pool, JobLease { job, holder: contender }).await,
        LeaseAcknowledgement::Applied,
        "the job existed, was leased by the contender, and was removed"
    );
    assert_eq!(
        settle_side_effect(&pool, JobLease { job, holder: contender }).await,
        LeaseAcknowledgement::Lost,
        "settling a removed job reaches no row a second time"
    );

    let remaining: i64 = sqlx::query_scalar("SELECT count(*) FROM pending_side_effects WHERE write_id = $1")
        .bind(write_id.as_uuid())
        .fetch_one(&pool)
        .await
        .expect("queue state after acknowledgement");
    assert_eq!(remaining, 0, "an acknowledged job leaves the queue");
    pool.close().await;
}

/// The claim state of the job `write_id` queued: its claimant, attempts,
/// recorded failure, when it is next due, and whether it is dead-lettered.
async fn job_state(pool: &PgPool, write_id: WriteId) -> (Option<uuid::Uuid>, i32, Option<String>, DateTime<Utc>, bool) {
    sqlx::query_as("SELECT claimed_by, attempt_count, last_error, next_attempt_at, exhausted_at IS NOT NULL FROM pending_side_effects WHERE write_id = $1")
        .bind(write_id.as_uuid())
        .fetch_one(pool)
        .await
        .expect("job state")
}

/// Releases are guarded on the job's claimant, not on the lease's end: once
/// another worker has reclaimed the lapsed job, the original worker is no
/// longer its claimant, so neither its retry release nor its dead-lettering
/// lands, and the reclaiming worker's claim is left exactly as it was.
#[tokio::test]
#[serial(durable_queue)]
async fn durable_queue_a_release_by_a_worker_whose_lapsed_job_was_reclaimed_is_refused_and_changes_nothing() {
    let pool = queue_database().await;
    let write_id = enqueue_side_effect(&pool).await;
    let repository = PendingSideEffectsRepository::new(&pool);
    let owner = side_effect_worker();
    let contender = side_effect_worker();
    let leased = repository.lease_batch(&owner, BATCH_SIZE, LEASE_SECONDS).await.expect("first lease");
    let job = leased.first().expect("the due job is leased").id;
    sleep(Duration::from_millis(1_500)).await;
    let reclaimed = repository.lease_batch(&contender, BATCH_SIZE, LEASE_SECONDS * 30).await.expect("reclaim");
    assert_eq!(reclaimed.len(), 1, "the lapsed job is reclaimed");
    let reclaimed_state = job_state(&pool, write_id).await;

    assert_eq!(
        repository
            .nack(job, &owner, "the original worker's replay failed", Utc::now() + RetentionDuration::minutes(5))
            .await
            .expect("the retry release runs"),
        LeaseAcknowledgement::Lost,
        "the original worker no longer holds the job it would release"
    );
    assert_eq!(
        repository
            .mark_dead(job, &owner, "the original worker gave up", DeadLetterAttempt::Failed)
            .await
            .expect("the dead-lettering runs"),
        LeaseAcknowledgement::Lost,
        "nor may it dead-letter it"
    );
    assert_eq!(
        job_state(&pool, write_id).await,
        reclaimed_state,
        "the refused releases left the reclaiming worker's claim untouched"
    );
    assert_eq!(
        reclaimed_state.0,
        Some(contender.as_uuid()),
        "the job stays claimed by the worker that reclaimed it"
    );
    pool.close().await;
}

#[tokio::test]
#[serial(durable_queue)]
async fn durable_queue_a_replayed_commit_enqueues_no_second_side_effect_job() {
    let pool = queue_database().await;
    let record = WriteRecord::mint(Utc::now());
    let payload = PersistedBatchSideEffectJob {
        temporal_entities: Vec::new(),
        change_events: Vec::new(),
    };
    for _ in 0..2 {
        let mut transaction = pool.begin().await.expect("enqueue transaction");
        PendingSideEffectsRepository::enqueue_in_tx(&mut transaction, &record, WriteOrdinal::at(0), &payload)
            .await
            .expect("enqueue under the same write identity");
        transaction.commit().await.expect("commit the enqueue");
    }

    let queued: i64 = sqlx::query_scalar("SELECT count(*) FROM pending_side_effects WHERE write_id = $1")
        .bind(record.write_id().as_uuid())
        .fetch_one(&pool)
        .await
        .expect("queued job count");
    assert_eq!(queued, 1, "a commit replayed under its original identity queues no second job");
    pool.close().await;
}

#[tokio::test]
#[serial(durable_queue)]
async fn durable_queue_a_side_effect_payload_that_cannot_be_decoded_reaches_a_coherent_dead_state() {
    let pool = queue_database().await;
    sqlx::query("INSERT INTO pending_side_effects (id, write_id, recorded_at, base_ordinal, payload) VALUES ($1, $2, $3, $4, $5)")
        .bind(SideEffectJobId::mint().as_uuid())
        .bind(WriteId::mint().as_uuid())
        .bind(Utc::now())
        .bind(0_i32)
        .bind(json!({"unreadable": true}))
        .execute(&pool)
        .await
        .expect("seed an undecodable job");
    let job = queued_job(&pool).await;

    let repository = PendingSideEffectsRepository::new(&pool);
    let leased = repository
        .lease_batch(&side_effect_worker(), BATCH_SIZE, LEASE_SECONDS)
        .await
        .expect("lease the undecodable job");
    assert!(leased.is_empty(), "a job nobody can decode is not handed to a worker");

    let (dead, lock_columns, last_error): (bool, i32, Option<String>) =
        sqlx::query_as("SELECT exhausted_at IS NOT NULL, num_nonnulls(lease_until, claimed_by), last_error FROM pending_side_effects WHERE id = $1")
            .bind(job.as_uuid())
            .fetch_one(&pool)
            .await
            .expect("dead-lettered job state");
    assert!(dead, "an undecodable job is retired rather than redelivered forever");
    assert_eq!(lock_columns, 0, "retiring the job releases the lease it was taken under");
    assert!(last_error.is_some(), "the decode failure is recorded against the retired job");

    let after = repository
        .lease_batch(&side_effect_worker(), BATCH_SIZE, LEASE_SECONDS)
        .await
        .expect("lease after the dead-lettering");
    assert!(after.is_empty(), "a dead job is never leased again");
    pool.close().await;
}

/// A dead-lettered job is evidence, kept for the configured retention and then
/// reaped by age; a live job, however old, is never touched by that reap.
#[tokio::test]
#[serial(durable_queue)]
async fn durable_queue_the_dead_letter_reap_removes_only_dead_jobs_older_than_the_cutoff() {
    let pool = queue_database().await;
    sqlx::query("INSERT INTO pending_side_effects (id, write_id, recorded_at, base_ordinal, payload) VALUES ($1, $2, $3, $4, $5)")
        .bind(SideEffectJobId::mint().as_uuid())
        .bind(WriteId::mint().as_uuid())
        .bind(Utc::now())
        .bind(0_i32)
        .bind(json!({"unreadable": true}))
        .execute(&pool)
        .await
        .expect("seed an undecodable job");
    enqueue_side_effect(&pool).await;

    let repository = PendingSideEffectsRepository::new(&pool);
    // Leasing retires the undecodable job; the decodable one is handed out and
    // stays live under its lease.
    let leased = repository
        .lease_batch(&side_effect_worker(), BATCH_SIZE, LEASE_SECONDS)
        .await
        .expect("lease the batch");
    assert_eq!(leased.len(), 1, "only the decodable job is handed to the worker");
    let dead_before: i64 = sqlx::query_scalar("SELECT count(*) FROM pending_side_effects WHERE exhausted_at IS NOT NULL")
        .fetch_one(&pool)
        .await
        .expect("dead job count");
    assert_eq!(dead_before, 1, "the undecodable job was dead-lettered");

    let too_young = repository
        .purge_dead_before(Utc::now() - RetentionDuration::hours(1))
        .await
        .expect("reap with a cutoff in the past");
    assert_eq!(too_young, 0, "a dead job younger than the retention is kept as evidence");

    let reaped = repository
        .purge_dead_before(Utc::now() + RetentionDuration::minutes(1))
        .await
        .expect("reap with a cutoff in the future");
    assert_eq!(reaped, 1, "a dead job past the retention is removed");
    let remaining: i64 = sqlx::query_scalar("SELECT count(*) FROM pending_side_effects")
        .fetch_one(&pool)
        .await
        .expect("remaining job count");
    assert_eq!(remaining, 1, "the live job survives the dead-letter reap whatever its age");
    pool.close().await;
}

/// The dead-letter retention runs from the instant a job gave up, not from
/// when it was enqueued: a job that retried for a month and was dead-lettered
/// an hour ago is still inside a week's retention, while one dead-lettered
/// eight days ago is past it.
#[tokio::test]
#[serial(durable_queue)]
async fn durable_queue_a_job_exhausted_after_the_retention_window_survives_the_reap() {
    let pool = queue_database().await;
    let now = Utc::now();
    let recently_exhausted = SideEffectJobId::mint();
    let long_exhausted = SideEffectJobId::mint();
    for (job, created_at, exhausted_at) in [
        (&recently_exhausted, now - RetentionDuration::days(30), now - RetentionDuration::hours(1)),
        (&long_exhausted, now - RetentionDuration::days(30), now - RetentionDuration::days(8)),
    ] {
        sqlx::query(
            "INSERT INTO pending_side_effects (id, write_id, recorded_at, base_ordinal, payload, created_at, exhausted_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(job.as_uuid())
        .bind(WriteId::mint().as_uuid())
        .bind(created_at)
        .bind(0_i32)
        .bind(json!({"unreadable": true}))
        .bind(created_at)
        .bind(exhausted_at)
        .execute(&pool)
        .await
        .expect("seed a dead-lettered job");
    }

    let repository = PendingSideEffectsRepository::new(&pool);
    let reaped = repository
        .purge_dead_before(now - RetentionDuration::days(7))
        .await
        .expect("reap with a week's retention");
    assert_eq!(reaped, 1, "only the job dead-lettered before the cutoff is removed");

    let survivors: Vec<uuid::Uuid> = sqlx::query_scalar("SELECT id FROM pending_side_effects")
        .fetch_all(&pool)
        .await
        .expect("surviving jobs");
    assert_eq!(
        survivors,
        vec![recently_exhausted.as_uuid()],
        "the recently dead-lettered job is kept whatever its age"
    );
    pool.close().await;
}
