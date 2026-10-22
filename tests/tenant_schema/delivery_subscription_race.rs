//! Delivery insert against a concurrent subscription deletion: the insert waits
//! the deletion out instead of failing the write.
//!
//! A write enqueues the notification deliveries it owes in its own
//! transaction, joined against `subscriptions` so a subscription deleted since
//! matching contributes no delivery. Under READ COMMITTED the join reads the
//! statement's snapshot, so a deletion that commits after that snapshot but
//! before the inserted row's foreign-key check would refuse the whole write --
//! a create failing because an unrelated request deleted a subscription, which
//! clause 5.6.1.4 does not allow. The join key-share locks the subscriptions it
//! keeps, so an in-flight deletion is waited out and the deleted subscription
//! then simply contributes nothing.
//!
//! The race is built deterministically: the deletion is left uncommitted, the
//! delivery insert is started against the same subscription, and the deletion
//! commits only once the catalogue shows the insert waiting on it.

use crate::SUITE;
use andromeda_context_commit::{subscription_key::SubscriptionKey, write_ordinal::WriteOrdinal, write_record::WriteRecord};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::entity_change::{EntityChange, EntityLifecycle};
use andromeda_postgresql_attribute_instance::name_keys::NameKeys;
use andromeda_postgresql_context_store::notification_delivery::{event_insert::insert_events_in_tx, rows::NewNotificationEvent};
use andromeda_postgresql_schema::{
    notification::{NotificationDeliveries, NotificationEvents},
    subscriptions::Subscriptions,
};
use andromeda_postgresql_seaquery::{aggregate::count_all, literal::int_literal, maintenance::backend_pid, statement::Statement};
use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::{database_access::connect_test_database, environment::TestEnvironment};
use chrono::Utc;
use reqwest::StatusCode;
use sea_query::{Alias, Expr, ExprTrait, Query};
use serde_json::json;
use sqlx::{PgPool, Postgres, Transaction};
use std::time::Duration;
use tokio::time::{sleep, timeout};

/// Whether backend `pid` is waiting for a lock.
async fn backend_waits(pool: &PgPool, pid: i32) -> bool {
    Query::select()
        .expr(Expr::exists(
            Query::select()
                .expr(int_literal(1))
                .from(Alias::new("pg_locks"))
                .and_where(Expr::col(Alias::new("pid")).eq(pid))
                .and_where(Expr::col(Alias::new("granted")).not())
                .to_owned(),
        ))
        .to_query_scalar()
        .fetch_one(pool)
        .await
        .expect("the lock catalogue must be readable")
}

/// Commit `deletion` once the delivery insert on backend `writer_pid` waits on it.
async fn commit_once_the_insert_waits(pool: &PgPool, writer_pid: i32, deletion: Transaction<'_, Postgres>) {
    timeout(Duration::from_secs(10), async {
        while !backend_waits(pool, writer_pid).await {
            sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the delivery insert must wait on the uncommitted subscription deletion");
    deletion.commit().await.expect("commit the subscription deletion");
}

#[tokio::test]
async fn delivery_insert_waits_out_a_concurrent_subscription_deletion_and_owes_it_nothing() {
    let scope = TestEnvironment::from_suite(SUITE).await.expect("isolated test environment");
    let pool = scope.database_pool();
    let subscription_id = scope.id("Subscription:delivery-race");
    let subscribed = scope
        .client()
        .create_subscription(&json!({
            "id": subscription_id,
            "type": "Subscription",
            "entities": [{"type": "DeliveryRaceProbe"}],
            "notification": {"endpoint": {"uri": "http://127.0.0.1:8085/notify", "accept": "application/json"}},
            "@context": [CORE_CONTEXT]
        }))
        .await
        .expect("create subscription request failed");
    pretty_assertions::assert_eq!(subscribed.status, StatusCode::CREATED, "{:?}", subscribed.body);
    let key: i64 = Query::select()
        .column(Subscriptions::SubscriptionKey)
        .from(Subscriptions::Table)
        .and_where(Expr::col(Subscriptions::Id).eq(subscription_id.as_str()))
        .to_query_scalar()
        .fetch_one(pool)
        .await
        .expect("subscription surrogate key");

    // The harness pool is two connections and the isolation lease holds one;
    // the two racing transactions take their own pool so the remaining one
    // stays free to poll the lock catalogue.
    let racing = connect_test_database().await.expect("racing connections");
    let mut deletion = racing.begin().await.expect("deletion transaction");
    let deleted = Query::delete()
        .from_table(Subscriptions::Table)
        .and_where(Expr::col(Subscriptions::SubscriptionKey).eq(key))
        .to_query()
        .execute(&mut *deletion)
        .await
        .expect("delete the subscription, uncommitted");
    pretty_assertions::assert_eq!(deleted.rows_affected(), 1);

    let mut write = racing.begin().await.expect("write transaction");
    let writer_pid: i32 = Query::select()
        .expr(backend_pid())
        .to_query_scalar()
        .fetch_one(&mut *write)
        .await
        .expect("writer backend pid");
    let entity_id: Identifier = scope.id("DeliveryRaceProbe:1").parse().expect("a valid entity identifier");
    let record = WriteRecord::mint(Utc::now());
    let change = EntityChange::new(EntityLifecycle::Created, Vec::new());
    let events = [NewNotificationEvent {
        ordinal: WriteOrdinal::at(0),
        change: &change,
        entity_id: &entity_id,
        change_attribute_name_keys: Vec::new(),
        entity_snapshot: None,
        previous_snapshot: None,
        sys_attrs_snapshot: None,
        subscription_keys: vec![SubscriptionKey::try_from(key).expect("a positive subscription key")],
    }];

    let keys = NameKeys::default();
    let insert = insert_events_in_tx(&mut write, &ContextTarget::Live, &record, &events, &keys);
    let (inserted, ()) = tokio::join!(insert, commit_once_the_insert_waits(pool, writer_pid, deletion));
    assert!(inserted.is_ok(), "the write must not fail on a subscription deleted under it: {inserted:?}");
    let owed: i64 = Query::select()
        .expr(count_all())
        .from(NotificationDeliveries::Table)
        .inner_join(
            NotificationEvents::Table,
            Expr::col((NotificationEvents::Table, NotificationEvents::EventKey)).equals((NotificationDeliveries::Table, NotificationDeliveries::EventKey)),
        )
        .and_where(Expr::col((NotificationEvents::Table, NotificationEvents::WriteId)).eq(record.write_id().as_uuid()))
        .to_query_scalar()
        .fetch_one(&mut *write)
        .await
        .expect("delivery count");
    pretty_assertions::assert_eq!(owed, 0, "the deleted subscription is owed no delivery");
    write.rollback().await.expect("discard the probe event");
    racing.close().await;

    scope.finish().await.expect("cleanup failed");
}
