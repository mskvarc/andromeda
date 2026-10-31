//! A query with a `limit` answers at most that many Context Source Registration Subscriptions (clause 5.11.5).

use crate::registration_subscription_operations::create_registration_subscription_triple;
use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink, problem::answered, teardown::AnyAnswer, wire::json_body};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::http::headers::Accept;
use serde_json::{Value, json};

/// Pins: Check that one can query context source registration subscriptions with a limit parameter and it will be
/// the maximum number of subscriptions to be retrieved
/// Clause: 5.11.5
/// Tenant: private (Query Context Source Registration Subscriptions lists every context source registration
/// subscription of the tenant, and the test compares the whole page with its own, or counts the subscriptions a limit
/// lets through; in the shared tenant every other file's subscriptions are listed too, so only a tenant of its own
/// keeps the comparison exact)
#[gtest]
#[tokio::test]
async fn e041_02_01_query_one_subscription() -> Result<()> {
    queried(1, |body| {
        let listed = body.as_array().map_or_else(|| vec![body], |items| items.iter().collect::<Vec<_>>());
        expect_that!(listed.len(), eq(1));
        expect_that!(
            listed.first().and_then(|subscription| subscription.get("type")),
            some(eq(&json!("Subscription")))
        );
    })
    .await
}

/// Pins: Check that one can query context source registration subscriptions with a limit parameter and it will be
/// the maximum number of subscriptions to be retrieved
/// Clause: 5.11.5
/// Tenant: private (Query Context Source Registration Subscriptions lists every context source registration
/// subscription of the tenant, and the test compares the whole page with its own, or counts the subscriptions a limit
/// lets through; in the shared tenant every other file's subscriptions are listed too, so only a tenant of its own
/// keeps the comparison exact)
#[gtest]
#[tokio::test]
async fn e041_02_02_query_two_subscription() -> Result<()> {
    queried(2, |body| {
        expect_that!(body.as_array().map(Vec::len), some(eq(2)));
        for pointer in ["/0/type", "/1/type"] {
            expect_that!(body.pointer(pointer), some(eq(&json!("Subscription"))));
        }
    })
    .await
}

/// Pins: Check that one can query context source registration subscriptions with a limit parameter and it will be
/// the maximum number of subscriptions to be retrieved
/// Clause: 5.11.5
/// Tenant: private (Query Context Source Registration Subscriptions lists every context source registration
/// subscription of the tenant, and the test compares the whole page with its own, or counts the subscriptions a limit
/// lets through; in the shared tenant every other file's subscriptions are listed too, so only a tenant of its own
/// keeps the comparison exact)
#[gtest]
#[tokio::test]
async fn e041_02_03_query_all_subscriptions() -> Result<()> {
    queried(15, |body| {
        expect_that!(body.as_array().map(Vec::len), some(eq(3)));
        for pointer in ["/0/type", "/1/type", "/2/type"] {
            expect_that!(body.pointer(pointer), some(eq(&json!("Subscription"))));
        }
    })
    .await
}

/// Sets up the three subscriptions in a tenant of the test's own, queries them with `limit` in the vocabulary's
/// `Link` context, requires 200, checks the body with `check`, and deletes the subscriptions.
async fn queried(limit: u32, check: impl FnOnce(&Value)) -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let tenant = broker::private_tenant(&broker, &namespace).await?;
    let sink = NotificationSink::start().await?;
    let (vocabulary, ids, _subscriptions) = create_registration_subscription_triple(&tenant, &namespace, &sink.endpoint(1)?).await?;

    let received = tenant
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .registration_subscriptions()
        .query()
        .limit(limit)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    check(&json_body(&received?)?);

    for id in &ids {
        tenant.registration_subscriptions().delete(id).send().await.any_answer();
    }
    Ok(())
}
