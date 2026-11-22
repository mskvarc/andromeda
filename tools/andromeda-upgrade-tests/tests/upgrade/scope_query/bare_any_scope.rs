//! A `scopeQ` v1.0.0 stored with the bare `/#` beside other Scopes, which
//! clause 4.19 admits only as a whole scope query, reads back after the
//! upgrade as the expression it meant, in a Subscription of the live branch,
//! in the queries a Snapshot was captured by and in a Subscription of a
//! Snapshot's branch. Every other `scopeQ` reads back as stored.

use crate::fixtures::{
    rehearsal::at_v1_0_0,
    snapshot_records::{insert_v1_0_0_snapshot, store_v1_0_0_snapshot_queries},
    statements::{insert, instant, json_document, smallint},
    test_error::UpgradeTestError,
};
use andromeda_configuration::schema::database::name_cache::NameCacheConfig;
use andromeda_context_space::{context_target::ContextTarget, snapshot_branch::SnapshotBranch};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
use andromeda_ngsi_ld_model::query::body::Query;
use andromeda_postgresql_context_store::{snapshot::repo::SnapshotRepository, subscription::repo::SubscriptionRepository};
use andromeda_postgresql_name_dictionary::name_cache::NameCache;
use andromeda_snapshot::store::SnapshotStore;
use andromeda_subscription::store::SubscriptionStore;
use pretty_assertions::assert_eq;
use sea_query::Expr;
use serde_json::{Value, json};
use sqlx::PgPool;
use std::sync::Arc;

/// Each `scopeQ` v1.0.0 stored, beside the one this build reads after the
/// upgrade.
///
/// `/#` beside other alternatives selects what `/#` alone selects, and inside
/// a group it adds nothing to the Scopes beside it, each of which already
/// requires a Scope.
const STORED_AND_READ: [(&str, &str); 9] = [
    ("/#,/CompanyA", "/#"),
    ("/CompanyA,/#", "/#"),
    ("(/#;/CompanyA),/CompanyB", "/CompanyA,/CompanyB"),
    ("(/CompanyA;/#;/Madrid/#)", "(/CompanyA;/Madrid/#)"),
    ("/CompanyB,(/#;/#)", "/#"),
    ("/#", "/#"),
    ("/Madrid/#", "/Madrid/#"),
    ("(/Madrid/#;/CompanyA)", "(/Madrid/#;/CompanyA)"),
    ("/Madrid/+/#,(/A;/B)", "/Madrid/+/#,(/A;/B)"),
];

/// The `q` every Snapshot query holds beside its `scopeQ`, which the rewrite
/// keeps as written.
const SNAPSHOT_Q: &str = "speed>50";

/// The Snapshot every Snapshot rehearsal stores.
const SNAPSHOT: &str = "urn:ngsi-ld:Snapshot:scoped";

/// Inserts the entity subscription `id` with the `scopeQ` `scope_q`, as v1.0.0
/// wrote it, into the branch `pool` reaches.
async fn insert_scoped_subscription(pool: &PgPool, id: &str, scope_q: &str) -> Result<(), UpgradeTestError> {
    insert(
        pool,
        "subscriptions",
        vec![
            ("id", Expr::val(id)),
            ("kind", smallint(1)),
            ("status", smallint(1)),
            ("created_at", instant("2025-01-27T21:00:00Z")),
            ("modified_at", instant("2025-01-27T21:00:00Z")),
            ("active_since", instant("2025-01-27T21:00:00Z")),
            ("selected_since", instant("2025-01-27T21:00:00Z")),
            (
                "notification",
                json_document(&json!({"endpoint": {"uri": "https://receiver.example.com/notify"}})),
            ),
            ("scope_q", Expr::val(scope_q)),
        ],
    )
    .await
}

/// The URI of the subscription storing the `index`th `scopeQ`.
fn subscription_id(index: usize) -> String {
    format!("urn:ngsi-ld:Subscription:scoped-{index}")
}

/// Inserts one subscription for each stored `scopeQ` into the branch `pool`
/// reaches.
async fn insert_scoped_subscriptions(pool: &PgPool) -> Result<(), UpgradeTestError> {
    for (index, (stored, _)) in STORED_AND_READ.iter().enumerate() {
        insert_scoped_subscription(pool, &subscription_id(index), stored).await?;
    }
    Ok(())
}

/// Expects each subscription of the branch `target` names to read back
/// through the broker's own repository with the `scopeQ` this build reads.
async fn expect_scoped_subscriptions_read_back(pool: &PgPool, target: &ContextTarget) -> Result<(), UpgradeTestError> {
    let attribute_names = Arc::new(NameCache::<AttributeName>::new(pool, &NameCacheConfig::default()));
    let store = SubscriptionRepository::new(pool, attribute_names);
    for (index, (stored, read)) in STORED_AND_READ.iter().enumerate() {
        let id = subscription_id(index).parse::<Identifier>()?;
        let subscription = store
            .get(target, &id)
            .await?
            .ok_or(UpgradeTestError::Missing { what: "a scoped subscription" })?
            .subscription;
        assert_eq!(
            subscription.filters.scope_q.map(|scope_q| scope_q.to_string()).as_deref(),
            Some(*read),
            "stored as {stored}"
        );
    }
    Ok(())
}

/// The clause 5.2.23 Queries v1.0.0 stored for a Snapshot, one for each
/// stored `scopeQ`, each beside the same `q`.
fn stored_snapshot_queries() -> Value {
    Value::Array(
        STORED_AND_READ
            .iter()
            .map(|(stored, _)| json!({"type": "Query", "q": SNAPSHOT_Q, "scopeQ": stored}))
            .collect(),
    )
}

/// Expects `queries` to hold one Query for each stored `scopeQ`, in order,
/// each with the `scopeQ` this build reads and its `q` as written.
fn expect_snapshot_queries_read_back(queries: Option<&[Query]>, member: &str) -> Result<(), UpgradeTestError> {
    let queries = queries.ok_or(UpgradeTestError::Missing {
        what: "the queries of a Snapshot",
    })?;
    assert_eq!(queries.len(), STORED_AND_READ.len(), "{member}");
    for (query, (stored, read)) in queries.iter().zip(STORED_AND_READ) {
        assert_eq!(
            query.scope_q.as_ref().map(ToString::to_string).as_deref(),
            Some(read),
            "{member} stored as {stored}"
        );
        assert_eq!(
            query.q.as_ref().map(ToString::to_string).as_deref(),
            Some(SNAPSHOT_Q),
            "{member} stored as {stored}"
        );
    }
    Ok(())
}

#[tokio::test]
async fn a_scope_query_of_a_live_subscription_holding_the_bare_any_scope_beside_other_scopes_reads_back_as_the_expression_it_meant()
-> Result<(), UpgradeTestError> {
    let (mut database, _) = at_v1_0_0().await?;
    insert_scoped_subscriptions(database.pool()).await?;

    database.migrate_to_current().await?;

    expect_scoped_subscriptions_read_back(database.pool(), &ContextTarget::Live).await?;
    database.drop_database().await?;
    Ok(())
}

#[tokio::test]
async fn a_scope_query_a_snapshot_was_captured_by_holding_the_bare_any_scope_beside_other_scopes_reads_back_as_the_expression_it_meant()
-> Result<(), UpgradeTestError> {
    let (mut database, _) = at_v1_0_0().await?;
    insert_v1_0_0_snapshot(&database, SNAPSHOT, SnapshotBranch::Published).await?;
    let stored = stored_snapshot_queries();
    store_v1_0_0_snapshot_queries(database.pool(), SNAPSHOT, &stored, &stored).await?;

    database.migrate_to_current().await?;

    let snapshot = SnapshotRepository::new(database.pool())
        .get(&SNAPSHOT.parse::<Identifier>()?)
        .await?
        .ok_or(UpgradeTestError::Missing { what: "the scoped Snapshot" })?;
    expect_snapshot_queries_read_back(snapshot.snapshot_queries.as_deref(), "snapshotQueries")?;
    expect_snapshot_queries_read_back(snapshot.snapshot_temporal_queries.as_deref(), "snapshotTemporalQueries")?;
    database.drop_database().await?;
    Ok(())
}

#[tokio::test]
async fn a_scope_query_of_a_snapshot_subscription_holding_the_bare_any_scope_beside_other_scopes_reads_back_as_the_expression_it_meant()
-> Result<(), UpgradeTestError> {
    let (mut database, _) = at_v1_0_0().await?;
    let key = insert_v1_0_0_snapshot(&database, SNAPSHOT, SnapshotBranch::Published).await?;
    let pool = database.branch_pool(key, SnapshotBranch::Published).await?;
    insert_scoped_subscriptions(&pool).await?;
    pool.close().await;

    database.migrate_to_current().await?;

    let target = SnapshotRepository::new(database.pool())
        .target(&SNAPSHOT.parse::<Identifier>()?)
        .await?
        .ok_or(UpgradeTestError::Missing {
            what: "the branch of the scoped Snapshot",
        })?;
    expect_scoped_subscriptions_read_back(database.pool(), &ContextTarget::Snapshot(target)).await?;
    database.drop_database().await?;
    Ok(())
}
