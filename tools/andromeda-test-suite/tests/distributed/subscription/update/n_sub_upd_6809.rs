//! A changed `datasetId` moves the copies to the sources whose registration shares it or lists none.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    dataset_copy_source::register_dataset_copy_source,
    forwarded_body,
    namespace::Namespace,
    notification_sink::NotificationSink,
    relayed_copy::placed_copy,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::Method;
use ngsi_ld::{
    model::{MemberChange, NgsiLdMediaType},
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionFragment},
};

/// Pins: Update Subscription replacing `datasetId` of dataset a with dataset b, over `federationOps` registrations of
/// the type listing a, listing b and listing none, deletes the copy at the registration listing a, places a copy
/// carrying b at the registration listing b, and patches the copy at the registration listing none with b
/// Clause: 5.12, 4.3.6.1, 5.2.9, 5.5.8, 5.8.1.4, 5.8.2.4, 5.11.3.4
#[gtest]
#[tokio::test]
async fn n_sub_upd_6809_a_changed_dataset_id_moves_the_copies_to_the_sources_whose_registration_shares_it_or_lists_none() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let source = ContextSource::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let subscription_id = namespace.subscription_id(1)?;
    let dataset_a = namespace.dataset_id("Dataset", "a")?;
    let dataset_b = namespace.dataset_id("Dataset", "b")?;
    register_dataset_copy_source(&broker, &source, &namespace, "s1", 1, Some(vec![dataset_a.clone()])).await?;
    register_dataset_copy_source(&broker, &source, &namespace, "s2", 2, Some(vec![dataset_b.clone()])).await?;
    register_dataset_copy_source(&broker, &source, &namespace, "s3", 3, None).await?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([vehicle_type.as_str()])
        .dataset_ids([dataset_a.clone()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    let _cleanup = Cleanup::of(&broker)
        .registration(&namespace.registration_id(1)?)
        .registration(&namespace.registration_id(2)?)
        .registration(&namespace.registration_id(3)?)
        .subscription(&subscription_id);
    broker.subscriptions().create(&subscription).send().await?;
    let copy_s1 = forwarded_body::subscription(&placed_copy(&source, "s1", &vehicle_type).await?)?;
    expect_that!(copy_s1.dataset_ids(), some(elements_are![eq(&dataset_a)]));
    placed_copy(&source, "s3", &vehicle_type).await?;
    // Copies are placed before Create Subscription answers, so a copy for the source listing b would have arrived by
    // now.
    expect_that!(source.received_on(Method::POST, "/s2/ngsi-ld/v1/subscriptions").await?, is_empty());
    let to_b = SubscriptionFragment::builder().dataset_ids([dataset_b.clone()]).build()?;
    broker.subscriptions().update(&subscription_id, &to_b).send().await?;
    let deleted = source
        .wait_for_matching(&SourceRequest::matching(Method::DELETE, "/s1/ngsi-ld/v1/subscriptions/.+"), 1)
        .await?;
    expect_that!(deleted, len(eq(1)));
    let copy_s2 = forwarded_body::subscription(&placed_copy(&source, "s2", &vehicle_type).await?)?;
    expect_that!(copy_s2.dataset_ids(), some(elements_are![eq(&dataset_b)]));
    let patched = source
        .wait_for_matching(&SourceRequest::matching(Method::PATCH, "/s3/ngsi-ld/v1/subscriptions/.+"), 1)
        .await?;
    expect_that!(patched, len(ge(1)));
    let patch = forwarded_body::subscription_fragment(patched.last().or_fail()?)?;
    let Some(MemberChange::Set(patched_ids)) = patch.dataset_ids() else {
        return fail!("the copy at the source listing none is not patched with a datasetId");
    };
    expect_that!(patched_ids, elements_are![eq(&dataset_b)]);
    Ok(())
}
