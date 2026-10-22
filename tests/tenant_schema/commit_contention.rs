//! Commit contention: a create whose commit is chosen as a deadlock victim
//! is run again rather than surfacing as a failure.
//!
//! PostgreSQL breaks a deadlock by rolling one contender's whole transaction
//! back. When the victim is a live write's commit, nothing of it persisted and
//! the identical write can run again, and clause 5.6.1.4 leaves a create
//! exactly one failure, `AlreadyExists`: a conflict with an unrelated
//! transaction must never surface as a failed create. The broker therefore
//! re-runs a commit that was chosen as the victim.
//!
//! The deadlock is built deterministically rather than raced for. A holder
//! session takes `attribute_names` in ACCESS EXCLUSIVE mode, then a create
//! with a type and an attribute no dictionary holds yet is sent: its commit
//! mints the type first -- taking ROW EXCLUSIVE on `entity_type_names` -- and
//! then waits on `attribute_names`. Once the catalogue shows exactly that wait,
//! the holder asks for `entity_type_names` in ACCESS EXCLUSIVE mode, closing
//! the cycle. The holder's `deadlock_timeout` is far longer than the server's,
//! so the broker's backend is the one that detects the cycle and is rolled
//! back; the holder's request is then granted, and the holder ends its
//! transaction, which lets the broker's second attempt through.

use crate::SUITE;
use andromeda_postgresql_schema::name_dictionary::{AttributeNames, EntityTypeNames};
use andromeda_postgresql_seaquery::{
    cast::cast,
    command::Command,
    introspection::current_database,
    literal::{int_literal, text_literal},
    maintenance::backend_pid,
    pg_type::PostgresType,
    relation_lock::{LockMode, RelationLock},
    scalar_subquery::scalar_subquery,
    setting::{Setting, SettingAssignment, SettingScope},
    statement::Statement,
    system_catalog::PgDatabase,
};
use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::{database_access::connect_test_database, serialized_environment::SerializedEnvironment};
use reqwest::StatusCode;
use sea_query::{Alias, Expr, ExprTrait, JoinType, Query, TableName};
use serde_json::json;
use sqlx::{PgPool, Postgres, Transaction};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::time::{sleep, timeout};

/// Whether a backend other than `holder_pid` holds ROW EXCLUSIVE on
/// `entity_type_names` while waiting for a lock on `attribute_names` -- the
/// commit halfway through minting its names.
async fn commit_waits_between_the_dictionaries(pool: &PgPool, holder_pid: i32) -> bool {
    let waiting = |column: &str| Expr::col((Alias::new("waiting"), Alias::new(column)));
    let held = |column: &str| Expr::col((Alias::new("held"), Alias::new(column)));
    let relation = |name: &str| cast(text_literal(name), PostgresType::Regclass);
    Query::select()
        .expr(Expr::exists(
            Query::select()
                .expr(int_literal(1))
                .from_as(Alias::new("pg_locks"), Alias::new("waiting"))
                .join_as(
                    JoinType::InnerJoin,
                    Alias::new("pg_locks"),
                    Alias::new("held"),
                    held("pid").equals((Alias::new("waiting"), Alias::new("pid"))),
                )
                .and_where(
                    waiting("database").eq(scalar_subquery(
                        Query::select()
                            .column(Alias::new("oid"))
                            .from(PgDatabase::Table)
                            .and_where(Expr::col(PgDatabase::Datname).eq(current_database()))
                            .to_owned(),
                    )),
                )
                .and_where(waiting("relation").eq(relation("attribute_names")))
                .and_where(waiting("granted").not())
                .and_where(held("relation").eq(relation("entity_type_names")))
                .and_where(held("granted"))
                .and_where(held("mode").eq("RowExclusiveLock"))
                .and_where(waiting("pid").ne(holder_pid))
                .to_owned(),
        ))
        .to_query_scalar()
        .fetch_one(pool)
        .await
        .expect("the lock catalogue must be readable")
}

/// Wait for the broker's commit to block between the two dictionaries, then
/// close the deadlock cycle and, once the broker's backend has been rolled
/// back, end the holder's transaction.
async fn close_the_cycle_then_release(pool: &PgPool, mut holder: Transaction<'_, Postgres>) {
    let holder_pid: i32 = Query::select()
        .expr(backend_pid())
        .to_query_scalar()
        .fetch_one(&mut *holder)
        .await
        .expect("holder backend pid");
    timeout(Duration::from_secs(10), async {
        while !commit_waits_between_the_dictionaries(pool, holder_pid).await {
            sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the create's commit must block on attribute_names while holding entity_type_names");
    // Granted only once the broker's backend has been chosen as the victim and
    // rolled back, releasing its ROW EXCLUSIVE lock.
    RelationLock::waiting(vec![TableName::from(EntityTypeNames::Table)], LockMode::AccessExclusive)
        .to_query()
        .execute(&mut *holder)
        .await
        .expect("the holder's lock is granted once the broker's commit is the victim");
    holder.rollback().await.expect("release both dictionaries");
}

#[tokio::test]
async fn commit_contention_a_create_chosen_as_deadlock_victim_is_committed_on_a_later_attempt() {
    let scope = SerializedEnvironment::from_environment_reset().await.expect("serialized test environment");
    let pool = scope.database_pool();
    let fresh = SystemTime::now().duration_since(UNIX_EPOCH).expect("clock after the epoch").as_nanos();
    let id = format!("urn:ngsi-ld:{SUITE}:DeadlockVictim:{fresh}");
    let entity = json!({
        "id": id,
        "type": format!("https://example.test/{SUITE}/DeadlockVictim{fresh}"),
        format!("https://example.test/{SUITE}/deadlockVictimSpeed{fresh}"): {"type": "Property", "value": 1},
        "@context": [CORE_CONTEXT]
    });

    // The harness pool is two connections and the serialized lease holds one;
    // the holder takes its own pool so the remaining one stays free to poll.
    let holding = connect_test_database().await.expect("holder connection");
    let mut holder = holding.begin().await.expect("holder transaction");
    SettingAssignment::new(Setting::DeadlockTimeout, "60s", SettingScope::Transaction)
        .to_query()
        .execute(&mut *holder)
        .await
        .expect("the holder never detects the cycle itself");
    RelationLock::waiting(vec![TableName::from(AttributeNames::Table)], LockMode::AccessExclusive)
        .to_query()
        .execute(&mut *holder)
        .await
        .expect("hold attribute_names");

    let create = async { scope.client().post_entity(&entity).with_ld_json().send().await.expect("create request failed") };
    let (created, ()) = tokio::join!(create, close_the_cycle_then_release(pool, holder));
    pretty_assertions::assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);

    let retrieved = scope
        .client()
        .get_entity(&id, "", "application/ld+json", None)
        .await
        .expect("retrieve request failed");
    pretty_assertions::assert_eq!(retrieved.status, StatusCode::OK);

    holding.close().await;
    scope.reset_and_finish().await.expect("cleanup failed");
}
