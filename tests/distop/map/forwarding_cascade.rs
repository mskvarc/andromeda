//! Tearing down forwarded subscription copies when the rows they hang off go (clause 5.8.5.4).
//!
//! The records of a reduced copy placed at a context source cascade with both the local subscription and the registration the copy was placed through, so the broker reads what it must tear down before either is deleted. Deleting either one removes the records and still sends the source the remote `DELETE`, whichever side went first.

use crate::{
    map::{
        csr_subscription::{FORWARD_TIMEOUT, subscription_source as source},
        subscription_document::{set_entity_type, subscription},
    },
    oracle::assert_status,
    topology::{self, BrokerRole},
};
use andromeda_postgresql_schema::subscription_forwarding::ForwardedSubscriptions;
use andromeda_postgresql_seaquery::{aggregate::count_all, statement::Statement};
use axum::http::Method;
use pretty_assertions::assert_eq;
use sea_query::{Expr, ExprTrait, Query};
use serde_json::Value;
use sqlx::PgPool;

fn scoped_sub(id: &str, entity_type: &str) -> Value {
    let mut body = subscription(id, "http://127.0.0.1:9/notify");
    set_entity_type(&mut body, entity_type);
    body
}

/// How many forwarded-copy records name `column = value`.
async fn forwarded_records(pool: &PgPool, column: ForwardedSubscriptions, value: &str) -> i64 {
    Query::select()
        .expr(count_all())
        .from(ForwardedSubscriptions::Table)
        .and_where(Expr::col(column).eq(value))
        .to_query_scalar()
        .fetch_one(pool)
        .await
        .expect("forwarded subscription records")
}

#[tokio::test]
async fn deleting_a_subscription_removes_its_forwarded_records_and_the_remote_copy() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("Vehicle");
    let mock = source(&topology, &vtype, "registration:cascade-subscription").await;
    let broker = topology.broker(BrokerRole::A).expect("topology broker A");
    let id = topology.id("Subscription:cascade-subscription");
    assert_status(
        &broker
            .client()
            .create_subscription(&scoped_sub(&id, &vtype))
            .await
            .expect("create subscription"),
        201,
    );
    let created = mock.next_request(FORWARD_TIMEOUT).await;
    assert_eq!(created.method, Method::POST, "the reduced copy is placed first");
    assert_eq!(forwarded_records(broker.pool(), ForwardedSubscriptions::LocalSubscriptionId, &id).await, 1);

    assert_status(&broker.client().delete_subscription(&id).await.expect("delete subscription"), 204);
    let torn_down = mock.next_request(FORWARD_TIMEOUT).await;
    assert_eq!(torn_down.method, Method::DELETE, "the source is told to delete its copy");
    assert!(torn_down.uri.path().contains("subscriptions"), "{}", torn_down.uri);
    assert_eq!(forwarded_records(broker.pool(), ForwardedSubscriptions::LocalSubscriptionId, &id).await, 0);

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

#[tokio::test]
async fn deleting_a_registration_removes_its_forwarded_records_and_the_remote_copy() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("Vehicle");
    let registration = topology.id("registration:cascade-registration");
    let mock = source(&topology, &vtype, "registration:cascade-registration").await;
    let broker = topology.broker(BrokerRole::A).expect("topology broker A");
    let id = topology.id("Subscription:cascade-registration");
    assert_status(
        &broker
            .client()
            .create_subscription(&scoped_sub(&id, &vtype))
            .await
            .expect("create subscription"),
        201,
    );
    let created = mock.next_request(FORWARD_TIMEOUT).await;
    assert_eq!(created.method, Method::POST, "the reduced copy is placed first");
    assert_eq!(forwarded_records(broker.pool(), ForwardedSubscriptions::RegistrationId, &registration).await, 1);

    assert_status(&broker.client().delete_registration(&registration).await.expect("delete registration"), 204);
    let torn_down = mock.next_request(FORWARD_TIMEOUT).await;
    assert_eq!(torn_down.method, Method::DELETE, "the source is told to delete the copy it holds");
    assert!(torn_down.uri.path().contains("subscriptions"), "{}", torn_down.uri);
    assert_eq!(forwarded_records(broker.pool(), ForwardedSubscriptions::RegistrationId, &registration).await, 0);

    assert_status(&broker.client().delete_subscription(&id).await.expect("delete subscription"), 204);
    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}
