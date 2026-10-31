//! A query lists every subscription of the tenant, each with the members it was created with (clause 5.8.4).

use crate::subscription_operations::create_query_subscriptions;
use andromeda_test_suite::{
    broker,
    comparison::matches_document,
    namespace::Namespace,
    notification_sink::NotificationSink,
    problem::answered,
    teardown::AnyAnswer,
    wire::json_body,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::http::headers::Accept;
use serde_json::json;

/// Pins: Check that one can query a list of subscriptions
/// Clause: 5.8.4
/// Tenant: private (Query Subscriptions lists every subscription of the tenant, and the test compares the whole first
/// page with its three subscriptions, no element left out; in the shared tenant every other file's subscriptions are
/// listed too, and the page Andromeda answers without a limit (the first 100 in identifier order) need not hold this
/// file's, so only a tenant of its own keeps the comparison exact)
#[gtest]
#[tokio::test]
async fn e031_01_01_query_subscriptions() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let tenant = broker::private_tenant(&broker, &namespace).await?;
    let sink = NotificationSink::start().await?;
    let endpoint_url = sink.endpoint(1)?;
    let (vocabulary, [first, second, third], _subscriptions) = create_query_subscriptions(&tenant, &namespace, &endpoint_url).await?;

    let listed = tenant
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .subscriptions()
        .query()
        .send()
        .await;
    verify_that!(&listed, answered(StatusCode::OK))?;
    let body = json_body(&listed?)?;
    let url = endpoint_url.as_str();
    expect_that!(
        &body,
        matches_document(json!([
            {
                "id": first.to_string(),
                "type": "Subscription",
                "timeInterval": 5,
                "entities": [{"type": "Building"}],
                "notification": {"format": "keyValues", "endpoint": {"uri": url, "accept": "application/json"}},
                "status": "active"
            },
            {
                "id": second.to_string(),
                "type": "Subscription",
                "watchedAttributes": ["name", "subCategory"],
                "entities": [{"type": "Building"}],
                "notification": {"format": "keyValues", "endpoint": {"uri": url, "accept": "application/json"}},
                "status": "active"
            },
            {
                "id": third.to_string(),
                "type": "Subscription",
                "entities": [{"type": "Building"}],
                "notification": {"format": "keyValues", "endpoint": {"uri": url, "accept": "application/json"}},
                "isActive": false,
                "status": "paused"
            }
        ]))
        .hiding_everywhere("jsonldContext")
        .hiding_everywhere("timesFailed")
        .hiding_everywhere("timesSent")
        .hiding_everywhere("notificationTrigger")
    );

    for id in [&first, &second, &third] {
        tenant.subscriptions().delete(id).send().await.any_answer();
    }
    Ok(())
}
