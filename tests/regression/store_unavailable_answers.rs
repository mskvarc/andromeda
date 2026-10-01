//! A store that is lost under a request is answered 503 with `Retry-After`.
//!
//! Clause 5.5.6 makes a failure the client did not cause, "database timeouts"
//! among them, an `InternalError`; clause 6.3.2 admits the HTTP binding's own
//! 503, which RFC 7231 section 6.6.4 defines for a condition that clears after
//! a delay `Retry-After` names. A backend PostgreSQL terminates under a
//! statement (SQLSTATE 57P01) is exactly that: nothing of the request is known
//! to have persisted, and it can be sent again.
//!
//! Each test holds an exclusive lock on the relation the statement under test
//! reads or writes, waits until the broker's backend queues behind it, and
//! terminates that backend. The registration lookup every batch write starts
//! with was answered as a context source failure (500); a write lost inside
//! the commit was answered with the pool-saturation detail.
//!
//! Naming convention: `regression_<finding>_<scenario>`.

use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::tenant_scoped_environment::TenantScopedEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};
use sqlx::PgPool;
use std::time::Duration;
use tokio::time::{Instant, sleep};

/// The detail of a store that cannot be reached.
const STORE_UNAVAILABLE: &str = "the datastore is temporarily unavailable; retry shortly";

/// How long the broker's backend may take to queue behind the lock.
const QUEUE_DEADLINE: Duration = Duration::from_secs(15);

/// The relation a test locks, and so the statement whose backend it cuts.
#[derive(Clone, Copy, Debug)]
enum LockedRelation {
    /// The tenant's registrations, read by the lookup every batch write starts
    /// with.
    Registrations,
    /// The live branch's entities, written inside the commit.
    Entities,
}

impl LockedRelation {
    /// The statement that takes the lock.
    const fn lock_statement(self) -> &'static str {
        match self {
            LockedRelation::Registrations => "LOCK TABLE public.context_source_registrations IN ACCESS EXCLUSIVE MODE",
            LockedRelation::Entities => "LOCK TABLE public.entities IN ACCESS EXCLUSIVE MODE",
        }
    }

    /// A pattern the text of a statement naming the relation matches.
    const fn statement_pattern(self) -> &'static str {
        match self {
            LockedRelation::Registrations => "%context_source_registrations%",
            LockedRelation::Entities => "%entities%",
        }
    }
}

/// One entity to upsert.
fn entity(scope: &TenantScopedEnvironment, local: &str) -> Value {
    json!([{ "id": scope.id(&format!("StoreProbe:{local}")), "type": "StoreProbe", "@context": CORE_CONTEXT.as_str() }])
}

/// Terminate every backend of this database that waits on a lock while
/// running a statement naming `relation`, once one does.
async fn terminate_the_backend_queued_on(database: &PgPool, relation: LockedRelation) {
    let deadline = Instant::now() + QUEUE_DEADLINE;
    loop {
        let terminated: Vec<bool> = sqlx::query_scalar(
            "SELECT pg_terminate_backend(pid) FROM pg_stat_activity \
             WHERE datname = current_database() AND pid <> pg_backend_pid() \
             AND wait_event_type = 'Lock' AND query ILIKE $1",
        )
        .bind(relation.statement_pattern())
        .fetch_all(database)
        .await
        .expect("the activity view is readable");
        if terminated.iter().any(|done| *done) {
            return;
        }
        assert!(Instant::now() < deadline, "no backend queued behind the lock on {relation:?}");
        sleep(Duration::from_millis(50)).await;
    }
}

/// Upsert one entity while `relation` is locked, terminate the backend that
/// queues behind the lock, and hand back the answer; then upsert it again once
/// the lock is gone and assert it lands.
async fn upsert_through_a_terminated_backend(scope: &TenantScopedEnvironment, relation: LockedRelation) -> (StatusCode, Option<String>, Value) {
    let client = scope.client();
    // Clause 5.5.10: creating an entity creates the tenant, so its runtime is
    // warm before the outage.
    let warm = client
        .post_entity(&json!({ "id": scope.id("StoreProbe:warm"), "type": "StoreProbe" }))
        .with_json(CORE_CONTEXT.as_str())
        .send()
        .await
        .expect("the warm-up create is answered");
    assert_eq!(
        warm.status,
        StatusCode::CREATED,
        "the tenant is warm before the outage: {:?}",
        warm.json_value()
    );

    let database = scope.tenant_database().await.expect("the tenant database exists");
    let mut lock = database.begin().await.expect("a transaction to hold the lock in");
    sqlx::query(relation.lock_statement())
        .execute(&mut *lock)
        .await
        .expect("the relation is locked");

    let probe = entity(scope, "probe");
    let (answer, ()) = tokio::join!(client.batch_upsert(&probe), terminate_the_backend_queued_on(&database, relation));
    let answer = answer.expect("the upsert is answered");
    lock.rollback().await.expect("the lock is released");

    let retried = client.batch_upsert(&probe).await.expect("the retried upsert is answered");
    assert_eq!(
        retried.status,
        StatusCode::CREATED,
        "the identical request lands once the store is back: {:?}",
        retried.json_value()
    );
    database.close().await;

    let retry_after = answer
        .headers
        .get("Retry-After")
        .map(|value| value.to_str().expect("a textual Retry-After").to_owned());
    let body = answer.json_value().expect("a problem details body");
    (answer.status, retry_after, body)
}

/// Assert the answer is the 503 of a store that cannot be reached.
fn assert_store_unavailable(status: StatusCode, retry_after: Option<&str>, body: &Value) {
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
    assert_eq!(retry_after, Some("5"), "{body}");
    assert_eq!(body["type"], "https://uri.etsi.org/ngsi-ld/errors/InternalError", "{body}");
    assert_eq!(body["detail"], STORE_UNAVAILABLE, "{body}");
    assert!(
        !body["detail"].as_str().unwrap_or_default().contains("context source"),
        "the broker's own store is not a context source: {body}"
    );
}

/// The registration lookup is the first statement of every batch write, so it
/// is what meets an outage first; it is this broker's own store failing.
#[tokio::test]
async fn regression_store_unavailable_a_registration_lookup_cut_by_a_terminated_backend_is_a_503_not_a_context_source_500() {
    let scope = TenantScopedEnvironment::from_suite("regression").await.expect("private tenant environment");
    let (status, retry_after, body) = upsert_through_a_terminated_backend(&scope, LockedRelation::Registrations).await;
    assert_store_unavailable(status, retry_after.as_deref(), &body);
    scope.finish().await.expect("cleanup failed");
}

/// A backend terminated under the commit is the store going away, not the
/// connection pool being saturated.
#[tokio::test]
async fn regression_store_unavailable_a_commit_cut_by_a_terminated_backend_is_a_503_not_a_commit_500() {
    let scope = TenantScopedEnvironment::from_suite("regression").await.expect("private tenant environment");
    let (status, retry_after, body) = upsert_through_a_terminated_backend(&scope, LockedRelation::Entities).await;
    assert_store_unavailable(status, retry_after.as_deref(), &body);
    scope.finish().await.expect("cleanup failed");
}
