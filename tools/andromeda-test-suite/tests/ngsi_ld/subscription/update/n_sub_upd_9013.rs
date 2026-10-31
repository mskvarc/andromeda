//! An empty attributes pick or omit in a notification fragment is bad request data and keeps the stored one.

use andromeda_test_suite::{broker, config::SuiteConfig, namespace::Namespace, problem::problem, subscription_document::with_notification_members};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::UpdateSubscription},
    model::{ErrorType, Input, NgsiLdMediaType},
    query::projection,
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionFragment},
};

/// Pins: Update Subscription whose notification fragment holds an empty attributes, pick or omit array answers 400
/// `BadRequestData` and keeps the stored notification, while a fragment holding one name in pick replaces it
/// Clause: 5.2.14, 5.5.4, 5.8.2.4
#[gtest]
#[tokio::test]
async fn n_sub_upd_9013_an_empty_attributes_pick_or_omit_in_a_notification_fragment_is_bad_request_data_and_keeps_the_stored_one() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let subscription_id = namespace.subscription_id(1)?;
    let building_type = namespace.entity_type("Building")?;
    let endpoint = || -> Result<Endpoint> { Ok(Endpoint::builder(format!("{unreachable}/notify")).build()?) };
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([building_type.as_str()])
        .notification(NotificationParams::builder(endpoint()?).attributes(["speed"]).build()?)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let fragment = SubscriptionFragment::builder()
        .notification(NotificationParams::<Input>::builder(endpoint()?).build()?)
        .build()?;
    for member in ["attributes", "pick", "omit"] {
        // The model builds no empty attributes, pick or omit, so the member is added to the written notification.
        let document = with_notification_members(&fragment.render(), &[(member, "[]")])?;
        let request = broker
            .unchecked(&UpdateSubscription {
                id: &subscription_id,
                fragment: &fragment,
            })?
            .document(&document);
        let refused = broker.send_unchecked(request).await;
        expect_that!(&refused, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    }
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let kept = json.subscriptions().retrieve(&subscription_id).send().await?;
    expect_that!(
        kept.notification().attributes().iter().map(ToString::to_string).collect::<Vec<_>>(),
        elements_are![eq("speed")]
    );
    expect_that!(kept.notification().pick(), none());
    expect_that!(kept.notification().omit(), none());
    let pick = projection::attributes(["name"]).build()?;
    let replacement = SubscriptionFragment::builder()
        .notification(NotificationParams::builder(endpoint()?).pick(pick.clone()).build()?)
        .build()?;
    broker.subscriptions().update(&subscription_id, &replacement).send().await?;
    let stored = json.subscriptions().retrieve(&subscription_id).send().await?;
    expect_that!(stored.notification().pick(), some(eq(&pick)));
    expect_that!(stored.notification().attributes(), is_empty());
    broker.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
