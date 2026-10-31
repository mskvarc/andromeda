//! A query lists every Context Source Registration Subscription of the tenant, each with the members it was created
//! with (clause 5.11.5).

use crate::registration_subscription_operations::create_registration_subscription_pair;
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

/// Pins: Check that one can query context source registration subscriptions
/// Clause: 5.11.5
/// Tenant: private (Query Context Source Registration Subscriptions lists every context source registration
/// subscription of the tenant, and the test compares the whole page with its own, or counts the subscriptions a limit
/// lets through; in the shared tenant every other file's subscriptions are listed too, so only a tenant of its own
/// keeps the comparison exact)
#[gtest]
#[tokio::test]
async fn e041_01_01_query_context_source_registration_subscriptions() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let tenant = broker::private_tenant(&broker, &namespace).await?;
    let sink = NotificationSink::start().await?;
    let uri = sink.endpoint(1)?;
    let (vocabulary, [first, second], _subscriptions) = create_registration_subscription_pair(&tenant, &namespace, &uri).await?;

    let listed = tenant
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .registration_subscriptions()
        .query()
        .send()
        .await;
    verify_that!(&listed, answered(StatusCode::OK))?;
    let body = json_body(&listed?)?;
    let url = uri.as_str();
    expect_that!(
        &body,
        matches_document(json!([
            {
                "id": first.to_string(),
                "type": "Subscription",
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
            }
        ]))
        .hiding_everywhere("jsonldContext")
        .hiding_everywhere("timesFailed")
        .hiding_everywhere("timesSent")
        .hiding_everywhere("notificationTrigger")
    );

    for id in [&first, &second] {
        tenant.registration_subscriptions().delete(id).send().await.any_answer();
    }
    Ok(())
}
