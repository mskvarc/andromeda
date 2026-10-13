//! Failing a side-effect replay on demand, at a chosen point in its lease.
//!
//! A session holding the relations a replay writes in SHARE mode keeps that
//! replay waiting inside its transaction, after it has already locked its
//! job; cancelling the waiting statement then fails the replay (SQLSTATE
//! 57014, which the committer reports as a persistence failure, not as
//! contention it would run again), and the drain releases the job.

use crate::queue_database::{enqueue_changes, owed_creation};
use andromeda_context_commit::write_id::WriteId;
use andromeda_entity::side_effect_drain::{EntitySideEffectDrain, EntitySideEffectDrainOutcome};
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use std::{sync::Arc, time::Duration};
use tokio::time::{Instant, sleep};

/// The longest a replay is waited for to reach the held lock.
const BLOCKED_DEADLINE: Duration = Duration::from_secs(10);

/// Queue one job owing the creation of `entity_id` to its subscribers, as a
/// respond-async batch defers it, and return the write it belongs to.
pub async fn enqueue_observation(pool: &PgPool, entity_id: &str) -> WriteId {
    enqueue_changes(pool, &[owed_creation(entity_id)]).await
}

/// Whether another session of the suite's database is waiting for a lock.
pub async fn a_session_waits_for_a_lock(pool: &PgPool) -> bool {
    sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM pg_stat_activity WHERE datname = current_database() AND wait_event_type = 'Lock')")
        .fetch_one(pool)
        .await
        .expect("session activity")
}

/// A pass of `drain` whose replay of a queued change is held for `held` once
/// it has reached the notification relations, and then fails.
///
/// Returns the pass's outcome and the instant the replay was failed at: any
/// release the drain recorded for it was recorded after that instant.
pub async fn pass_with_failed_replay(pool: &PgPool, drain: &Arc<EntitySideEffectDrain>, held: Duration) -> (EntitySideEffectDrainOutcome, DateTime<Utc>) {
    let mut holder = pool.begin().await.expect("lock holder transaction");
    sqlx::query("LOCK TABLE notification_events, notification_deliveries IN SHARE MODE")
        .execute(&mut *holder)
        .await
        .expect("hold the notification relations");
    let passing = Arc::clone(drain);
    let pass = tokio::spawn(async move { passing.execute(Utc::now()).await });
    let deadline = Instant::now() + BLOCKED_DEADLINE;
    while !a_session_waits_for_a_lock(pool).await {
        assert!(Instant::now() < deadline, "the replay reaches the held lock before the deadline");
        sleep(Duration::from_millis(20)).await;
    }
    sleep(held).await;
    let failed_at = Utc::now();
    let cancelled: bool =
        sqlx::query_scalar("SELECT bool_and(pg_cancel_backend(pid)) FROM pg_stat_activity WHERE datname = current_database() AND wait_event_type = 'Lock'")
            .fetch_one(pool)
            .await
            .expect("cancel the waiting replay");
    assert!(cancelled, "the waiting replay is cancelled");
    holder.rollback().await.expect("release the notification relations");
    let outcome = pass.await.expect("the drain pass ran to completion").expect("the drain pass reports no error");
    (outcome, failed_at)
}
