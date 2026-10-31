//! An empty fragment answers 204 and changes nothing.

use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink, problem::answered, written_json::json_request_to};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::{headers::Accept, operation::SystemAttributes},
    model::NgsiLdMediaType,
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Update Context Source Registration Subscription with the empty Fragment {} as application/json answers 204
/// and the subscription keeps its members and its `modifiedAt`
/// Clause: 5.4, 5.11.3, 4.8
/// Recorded: clause 5.4 makes a Fragment an IETF RFC 7396 JSON Merge Patch, under which the empty patch leaves its
/// target unchanged, and no clause lists an empty Fragment among the errors of the update
#[gtest]
#[tokio::test]
async fn n_css_upd_2701_an_empty_fragment_answers_204_and_changes_nothing() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let building_type = namespace.entity_type("Building")?;
    let subscription = Subscription::registration_builder()
        .id(subscription_id.clone())
        .description("before")
        .entities([building_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.registration_subscriptions().create(&subscription).send().await?;
    let before = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registration_subscriptions()
        .retrieve(&subscription_id)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    let modified = before.output().modified_at;
    expect_that!(modified, some(anything()));
    // The empty fragment carries no member, which no typed fragment writes, so it is sent as written.
    let request = json_request_to(&broker, Method::PATCH, &format!("csourceSubscriptions/{subscription_id}"), "{}")?;
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::NO_CONTENT));
    let after = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registration_subscriptions()
        .retrieve(&subscription_id)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    expect_that!(after.description(), some(eq("before")));
    expect_that!(after.output().modified_at, eq(modified));
    broker.registration_subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
