//! An update fragment naming an unknown member is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::UpdateSubscription},
    model::{ErrorType, NgsiLdMediaType, RawJson},
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionFragment},
};

/// Pins: Update Subscription with a fragment naming a member outside Table 5.2.12-1 answers 400 `BadRequestData`
/// naming the member and leaves the stored subscription unchanged
/// Clause: 5.2.12, 5.8.2.4
/// Recorded: the refusal is Andromeda's own bound, stricter than the clause; a misspelt member of a fragment would
/// otherwise be dropped and the update answered 204 as if it had been applied
#[gtest]
#[tokio::test]
async fn a_sub_upd_9008_an_update_fragment_naming_an_unknown_member_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_unknown_update_1_id = namespace.subscription_id("unknown-update-1")?;
    let building_type = namespace.entity_type("Building")?;
    let subscription_unknown_update_1 = Subscription::builder()
        .id(subscription_unknown_update_1_id.clone())
        .subscription_name("before")
        .entities([building_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("unknown-update-1")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription_unknown_update_1).send().await?;
    let fragment = SubscriptionFragment::builder().subscription_name("after").build()?;
    // The model refuses it (member `watchedAtributes` is not allowed here), so `watchedAtributes` is written as text.
    let watched_atributes: RawJson = r#"["name"]"#.parse()?;
    let document = UncheckedDocument::try_from(fragment.render())?.set_member("watchedAtributes", &watched_atributes);
    let request = broker
        .unchecked(&UpdateSubscription {
            id: &subscription_unknown_update_1_id,
            fragment: &fragment,
        })?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(
        &answer,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail_containing("watchedAtributes")
    );
    let subscription = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_unknown_update_1_id)
        .send()
        .await?;
    expect_that!(subscription.name(), some(eq("before")));
    expect_that!(subscription.watched_attributes(), is_empty());
    Ok(())
}
