//! An empty fragment moves no modified at.

use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink, problem::answered, written_json::json_request_to};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::{headers::Accept, operation::SystemAttributes},
    model::NgsiLdMediaType,
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Update Subscription with the empty Fragment {} leaves the Subscription's `modifiedAt` as it was
/// Clause: 4.8, 5.4, 5.8.2
/// Recorded: an empty write modifies nothing, so nothing has a new modification time (clause 4.8)
#[gtest]
#[tokio::test]
async fn n_sub_upd_2701_an_empty_fragment_moves_no_modified_at() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let building_type = namespace.entity_type("Building")?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .description("before")
        .entities([building_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let before = json
        .subscriptions()
        .retrieve(&subscription_id)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    let modified = before.output().modified_at.or_fail()?;
    // An empty fragment is no value the fragment model distinguishes from a missing one, so it is sent as written.
    let emptied = broker
        .send_unchecked(json_request_to(&broker, Method::PATCH, &format!("subscriptions/{subscription_id}"), "{}")?)
        .await;
    expect_that!(&emptied, answered(StatusCode::NO_CONTENT));
    let after = json
        .subscriptions()
        .retrieve(&subscription_id)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    expect_that!(after.description(), some(eq("before")));
    expect_that!(after.output().modified_at, some(eq(modified)));
    broker.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
