//! Exclusive ownership of a due side-effect job.
//!
//! Verification-matrix row "Side-effect exclusive lease": two workers run
//! against one due row, the owner crashes, and the clock advances past
//! `locked_until`. Only one initial lease exists, the row is reclaimed after
//! expiry, acknowledgement removes it once, a payload that cannot be decoded
//! reaches a coherent dead state, and a dead row is reaped once its retention
//! has passed while a live row is not.

use crate::{
    queue_database::{live_target, queue_database},
    queue_workers::side_effect_worker,
};
use andromeda_context_commit::{
    lease_acknowledgement::LeaseAcknowledgement,
    side_effect_job_id::SideEffectJobId,
    write_id::WriteId,
    write_ordinal::WriteOrdinal,
    write_record::WriteRecord,
};
use andromeda_entity::side_effect_queue::SideEffectQueue;
use andromeda_postgresql_context_store::pending_side_effects::{payload::PersistedBatchSideEffectJob, repo::PendingSideEffectsRepository};
use chrono::{Duration as RetentionDuration, Utc};
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

/// Enqueue one decodable job through the committer's own enqueue statement.
async fn enqueue(pool: &PgPool) -> WriteId {
    let record = WriteRecord::mint(Utc::now());
    let payload = PersistedBatchSideEffectJob {
        temporal_entities: Vec::new(),
        change_events: Vec::new(),
    };
    let mut transaction = pool.begin().await.expect("enqueue transaction");
    PendingSideEffectsRepository::enqueue_in_tx(&mut transaction, &live_target(pool).await, &record, WriteOrdinal::at(0), &payload)
        .await
        .expect("enqueue the job");
    transaction.commit().await.expect("commit the enqueue");
    record.write_id()
}

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
    let write_id = enqueue(&pool).await;
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
        repository.ack(job, &contender).await.expect("first acknowledgement"),
        LeaseAcknowledgement::Applied,
        "the job existed, was leased by the contender, and was removed"
    );
    assert_eq!(
        repository.ack(job, &contender).await.expect("second acknowledgement"),
        LeaseAcknowledgement::Lost,
        "acknowledging a removed job reaches no row a second time"
    );

    let remaining: i64 = sqlx::query_scalar("SELECT count(*) FROM pending_side_effects WHERE write_id = $1")
        .bind(write_id.as_uuid())
        .fetch_one(&pool)
        .await
        .expect("queue state after acknowledgement");
    assert_eq!(remaining, 0, "an acknowledged job leaves the queue");
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
        PendingSideEffectsRepository::enqueue_in_tx(&mut transaction, &live_target(&pool).await, &record, WriteOrdinal::at(0), &payload)
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
    let target = live_target(&pool).await;
    sqlx::query("INSERT INTO pending_side_effects (context_space_key, id, write_id, recorded_at, consequence_base, payload) VALUES ($1, $2, $3, $4, $5, $6)")
        .bind(target.key().as_i64())
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
        sqlx::query_as("SELECT dead, num_nonnulls(locked_until, locked_by), last_error FROM pending_side_effects WHERE id = $1")
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
    let target = live_target(&pool).await;
    sqlx::query("INSERT INTO pending_side_effects (context_space_key, id, write_id, recorded_at, consequence_base, payload) VALUES ($1, $2, $3, $4, $5, $6)")
        .bind(target.key().as_i64())
        .bind(SideEffectJobId::mint().as_uuid())
        .bind(WriteId::mint().as_uuid())
        .bind(Utc::now())
        .bind(0_i32)
        .bind(json!({"unreadable": true}))
        .execute(&pool)
        .await
        .expect("seed an undecodable job");
    enqueue(&pool).await;

    let repository = PendingSideEffectsRepository::new(&pool);
    // Leasing retires the undecodable job; the decodable one is handed out and
    // stays live under its lease.
    let leased = repository
        .lease_batch(&side_effect_worker(), BATCH_SIZE, LEASE_SECONDS)
        .await
        .expect("lease the batch");
    assert_eq!(leased.len(), 1, "only the decodable job is handed to the worker");
    let dead_before: i64 = sqlx::query_scalar("SELECT count(*) FROM pending_side_effects WHERE dead")
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
