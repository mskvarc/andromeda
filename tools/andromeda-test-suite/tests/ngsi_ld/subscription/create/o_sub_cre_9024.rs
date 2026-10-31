//! A subscription read back and written again is accepted.

use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink, problem::answered};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        UncheckedDocument,
        headers::Accept,
        operation::{SystemAttributes, UpdateSubscription},
    },
    model::{NgsiLdMediaType, RawJson},
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionFragment},
};

/// Pins: a Subscription read back with its generated members (status, `createdAt`, `modifiedAt`, and the
/// notification members `timesSent` and status) is accepted again by an update, which ignores them
/// Clause: 5.2.2, 5.2.12, 5.2.14.2, 5.8.2.4
/// Recorded: "NGSI-LD implementations shall ignore them" for the members a Context Subscriber shall not provide, so
/// refusing unknown members does not break the read-then-write round trip
#[gtest]
#[tokio::test]
async fn o_sub_cre_9024_a_subscription_read_back_and_written_again_is_accepted() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id("round-trip-1")?;
    let building_type = namespace.entity_type("Building")?;
    let endpoint = sink.endpoint("round-trip-1")?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .subscription_name("round trip")
        .entities([building_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(endpoint.clone()).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let with_system_attributes = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_id)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    expect_that!(with_system_attributes.output().created_at, some(anything()));
    expect_that!(with_system_attributes.output().modified_at, some(anything()));
    let again = Subscription::builder()
        .id(subscription_id.clone())
        .subscription_name("round trip written again")
        .entities([building_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(endpoint.clone()).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    // The model writes no member only a broker provides, so those members are set on the rendered document.
    let generated: RawJson = r#""active""#.parse()?;
    let instant: RawJson = r#""2024-01-01T00:00:00Z""#.parse()?;
    let notification: RawJson = format!(
        r#"{{
        "endpoint": {{"uri": "{endpoint}", "accept": "application/json", "createdAt": "2024-01-01T00:00:00Z"}},
        "status": "ok",
        "timesSent": 3,
        "timesFailed": 0,
        "lastNotification": "2024-01-01T00:00:00Z",
        "lastSuccess": "2024-01-01T00:00:00Z"
    }}"#
    )
    .parse()?;
    let document = UncheckedDocument::try_from(again.render())?
        .set_member("status", &generated)
        .set_member("createdAt", &instant)
        .set_member("modifiedAt", &instant)
        .set_member("notification", &notification);
    let fragment = SubscriptionFragment::builder().build()?;
    let request = broker
        .unchecked(&UpdateSubscription {
            id: &subscription_id,
            fragment: &fragment,
        })?
        .document(&document);
    let written_again = broker.send_unchecked(request).await;
    expect_that!(&written_again, answered(StatusCode::NO_CONTENT));
    let stored = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await?;
    expect_that!(stored.name(), some(eq("round trip written again")));
    broker.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
