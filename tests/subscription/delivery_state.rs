//! The delivery counters of a subscription (clause 5.2.15) live in their own
//! row, `subscription_delivery_state`, created with the subscription, moved by
//! every notification and reclaimed with the subscription; the subscription
//! row itself is never written by a delivery.

use crate::notification_adapter::NotificationAdapter;
use andromeda_postgresql_schema::subscriptions::{SubscriptionDeliveryState, Subscriptions};
use andromeda_postgresql_seaquery::{
    aggregate::count_all,
    introspection::current_schema,
    statement::Statement,
    system_catalog::{InformationSchema, InformationSchemaColumns},
};
use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use chrono::{DateTime, Utc};
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use sea_query::{Expr, ExprTrait, Query};
use serde_json::{Value, json};
use sqlx::PgPool;
use std::time::Duration;
use tokio::time::sleep;

/// The suite name the isolated environments of this module are allocated under.
const SUITE: &str = "subscription";

/// A subscription over every `Building` notifying `endpoint`.
fn building_subscription(id: &str, endpoint: &str) -> Value {
    json!({
        "id": id,
        "type": "Subscription",
        "entities": [{"type": "Building"}],
        "notification": {
            "format": "normalized",
            "endpoint": {"uri": endpoint, "accept": "application/json"}
        },
        "@context": [CORE_CONTEXT]
    })
}

/// A `Building` with one property to append to.
fn building(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Building",
        "name": {"type": "Property", "value": "Hall"},
        "@context": [CORE_CONTEXT]
    })
}

/// The state row of `subscription_id`: `(times_sent, last_notification,
/// last_success)`, `None` when no row exists.
async fn delivery_state(pool: &PgPool, subscription_id: &str) -> Option<(i32, Option<DateTime<Utc>>, Option<DateTime<Utc>>)> {
    Query::select()
        .columns([
            (SubscriptionDeliveryState::Table, SubscriptionDeliveryState::TimesSent),
            (SubscriptionDeliveryState::Table, SubscriptionDeliveryState::LastNotification),
            (SubscriptionDeliveryState::Table, SubscriptionDeliveryState::LastSuccess),
        ])
        .from(SubscriptionDeliveryState::Table)
        .inner_join(
            Subscriptions::Table,
            Expr::col((Subscriptions::Table, Subscriptions::SubscriptionKey))
                .equals((SubscriptionDeliveryState::Table, SubscriptionDeliveryState::SubscriptionKey)),
        )
        .and_where(Expr::col((Subscriptions::Table, Subscriptions::Id)).eq(subscription_id))
        .to_query_as()
        .fetch_optional(pool)
        .await
        .expect("the delivery state must be readable")
}

/// The `(created_at, modified_at)` instants of the subscription row.
async fn lifecycle_instants(pool: &PgPool, subscription_id: &str) -> (DateTime<Utc>, DateTime<Utc>) {
    Query::select()
        .columns([Subscriptions::CreatedAt, Subscriptions::ModifiedAt])
        .from(Subscriptions::Table)
        .and_where(Expr::col(Subscriptions::Id).eq(subscription_id))
        .to_query_as()
        .fetch_one(pool)
        .await
        .expect("the subscription row must be readable")
}

#[tokio::test]
async fn creating_a_subscription_creates_its_delivery_state_row_with_zero_counters() {
    let scope = TestEnvironment::from_suite(SUITE).await.expect("isolated test environment");
    let subscription_id = scope.id("urn:ngsi-ld:Subscription:state-created");

    let created = scope
        .client()
        .create_subscription(&building_subscription(&subscription_id, "http://127.0.0.1:8085/notify"))
        .await
        .expect("the create request must be sent");
    assert_eq!(created.status, StatusCode::CREATED);

    assert_eq!(delivery_state(scope.database_pool(), &subscription_id).await, Some((0, None, None)));

    let counter_columns: i64 = Query::select()
        .expr(count_all())
        .from((InformationSchema, InformationSchemaColumns::Table))
        .and_where(Expr::col(InformationSchemaColumns::TableSchema).eq(current_schema()))
        .and_where(Expr::col(InformationSchemaColumns::TableName).eq("subscriptions"))
        .and_where(Expr::col(InformationSchemaColumns::ColumnName).is_in(["times_sent", "last_notification", "last_success", "last_failure"]))
        .to_query_scalar()
        .fetch_one(scope.database_pool())
        .await
        .expect("the catalogue must be readable");
    assert_eq!(counter_columns, 0, "the subscription row must carry no delivery counter");

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn deleting_a_subscription_reclaims_its_delivery_state_row() {
    let scope = TestEnvironment::from_suite(SUITE).await.expect("isolated test environment");
    let subscription_id = scope.id("urn:ngsi-ld:Subscription:state-deleted");

    let created = scope
        .client()
        .create_subscription(&building_subscription(&subscription_id, "http://127.0.0.1:8085/notify"))
        .await
        .expect("the create request must be sent");
    assert_eq!(created.status, StatusCode::CREATED);
    assert!(delivery_state(scope.database_pool(), &subscription_id).await.is_some());

    let deleted = scope
        .client()
        .delete_subscription(&subscription_id)
        .await
        .expect("the delete request must be sent");
    assert_eq!(deleted.status, StatusCode::NO_CONTENT);
    assert_eq!(delivery_state(scope.database_pool(), &subscription_id).await, None);

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn a_delivered_notification_moves_the_state_row_and_leaves_the_subscription_row_unmodified() {
    let scope = TestEnvironment::from_suite(SUITE).await.expect("isolated test environment");
    let receiver = NotificationAdapter::start().await;
    let subscription_id = scope.id("urn:ngsi-ld:Subscription:state-moved");
    let entity_id = scope.id("urn:ngsi-ld:Building:state-moved");

    let created = scope
        .client()
        .create_subscription(&building_subscription(&subscription_id, &receiver.endpoint_url()))
        .await
        .expect("the create request must be sent");
    assert_eq!(created.status, StatusCode::CREATED);
    let (created_at, modified_before) = lifecycle_instants(scope.database_pool(), &subscription_id).await;

    let entity = scope
        .client()
        .post_entity(&building(&entity_id))
        .send()
        .await
        .expect("the entity create must be sent");
    assert_eq!(entity.status, StatusCode::CREATED);
    let _ = receiver.wait_for_subscription_notification(&subscription_id, Duration::from_secs(10)).await;

    let mut state = delivery_state(scope.database_pool(), &subscription_id).await;
    for _ in 0..40 {
        if matches!(state, Some((sent, Some(_), Some(_))) if sent >= 1) {
            break;
        }
        sleep(Duration::from_millis(250)).await;
        state = delivery_state(scope.database_pool(), &subscription_id).await;
    }
    let (times_sent, last_notification, last_success) = state.expect("the state row must exist");
    assert!(times_sent >= 1, "the claim must have counted the notification: {times_sent}");
    assert!(
        last_notification.is_some() && last_success.is_some(),
        "the claim and the outcome must both be stamped"
    );

    let (_, modified_after) = lifecycle_instants(scope.database_pool(), &subscription_id).await;
    assert_eq!(
        modified_after, modified_before,
        "a delivery must not move the subscription's modification instant"
    );
    assert_eq!(modified_after, created_at, "an unmodified subscription reports its creation instant");

    let retrieved = scope
        .client()
        .retrieve_subscription(&subscription_id, None)
        .await
        .expect("the retrieve request must be sent");
    assert_eq!(retrieved.status, StatusCode::OK);
    let body = retrieved.json_value().expect("the subscription must render as JSON");
    assert_eq!(
        body["notification"]["timesSent"],
        json!(times_sent),
        "the resource reports the state row's counter"
    );
    assert_eq!(body["notification"]["status"], "ok");

    receiver.shutdown().await;
    scope.finish().await.expect("cleanup failed");
}
