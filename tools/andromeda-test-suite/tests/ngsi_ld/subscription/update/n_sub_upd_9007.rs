//! A json null is bad request data and the ngsi ld null removes the member.

use andromeda_test_suite::{broker, config::SuiteConfig, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, UncheckedRequest, headers::Accept, operation::UpdateSubscription},
    model::{ErrorType, NgsiLdMediaType, RawJson},
    subscription::{Activation, Endpoint, NotificationParams, Subscription, SubscriptionFragment, SubscriptionMember, SubscriptionStatus},
};

/// Pins: Update Subscription with throttling null answers 400 `BadRequestData` naming throttling and keeps it; with
/// throttling, `isActive` and description set to urn:ngsi-ld:null answers 204 and reads back without throttling,
/// description and `isActive` and with status active; with notification set to urn:ngsi-ld:null answers 400 `BadRequestData` and keeps the
/// notification
/// Clause: 4.5.0, 5.2.1, 5.5.8, 5.8.2.4, Table 5.2.12-1
#[gtest]
#[tokio::test]
async fn n_sub_upd_9007_a_json_null_is_bad_request_data_and_the_ngsi_ld_null_removes_the_member() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let subscription_id = namespace.subscription_id(1)?;
    let probe_type = namespace.entity_type("NullProbe")?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([probe_type.as_str()])
        .description("throttled")
        .throttling(5)
        .activation(Activation::Paused)
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let empty = SubscriptionFragment::builder().build()?;
    let patch = |document: &UncheckedDocument| -> Result<UncheckedRequest> {
        Ok(broker
            .unchecked(&UpdateSubscription {
                id: &subscription_id,
                fragment: &empty,
            })?
            .document(document))
    };
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    // A JSON null and the NGSI-LD Null in a notification are no changes the fragment model writes, so those members are set as given.
    let json_null: RawJson = "null".parse()?;
    let throttling_null = UncheckedDocument::try_from(empty.render())?.set_member("throttling", &json_null);
    let refused = broker.send_unchecked(patch(&throttling_null)?).await;
    expect_that!(
        &refused,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail_containing("throttling")
    );
    let kept = json.subscriptions().retrieve(&subscription_id).send().await?;
    expect_that!(kept.throttling().map(ToString::to_string), some(eq("5")));
    let notification_null = UncheckedDocument::try_from(empty.render())?.null_member("notification");
    let refused_2 = broker.send_unchecked(patch(&notification_null)?).await;
    expect_that!(&refused_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let removal = SubscriptionFragment::builder()
        .delete(SubscriptionMember::Throttling)
        .delete(SubscriptionMember::IsActive)
        .delete(SubscriptionMember::Description)
        .build()?;
    broker.subscriptions().update(&subscription_id, &removal).send().await?;
    let stored = json.subscriptions().retrieve(&subscription_id).send().await?;
    expect_that!(stored.throttling(), none());
    expect_that!(stored.description(), none());
    // Table 5.2.12-1 makes `isActive` optional with true as its default, so a removed `isActive` reads back absent, and
    // the Subscription is active (clause 5.8.2.4).
    expect_that!(stored.activation(), eq(Activation::Unspecified));
    expect_that!(stored.output().status, some(eq(SubscriptionStatus::Active)));
    expect_that!(stored.notification().endpoint().uri().as_str(), eq(format!("{unreachable}/notify").as_str()));
    broker.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
