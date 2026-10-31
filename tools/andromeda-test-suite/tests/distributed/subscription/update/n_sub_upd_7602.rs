//! Watched attribute terms of an update are stored expanded.

use andromeda_test_suite::{
    broker,
    core_context::core_entry,
    hosted_context::HostedContext,
    namespace::Namespace,
    notification_sink::NotificationSink,
    nth::nth,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{AttributeName, Context, ContextEntry, InlineContext, NgsiLdMediaType, Representation},
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionFragment},
};

/// Pins: Update Subscription on broker A with `watchedAttributes` speed under an @context mapping speed to a test
/// IRI answers 204, and the subscription reads back that IRI under the core context alone
/// Clause: 5.8.2.4, 5.5.7, 5.2.12
#[gtest]
#[tokio::test]
async fn n_sub_upd_7602_watched_attribute_terms_of_an_update_are_stored_expanded() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let context = Context::Multiple(vec![
        ContextEntry::Inline(format!(r#"{{"speed": "https://test.andromeda/{namespace}/speed"}}"#).parse::<InlineContext>()?),
        core_entry()?,
    ]);
    let ctx = HostedContext::add(&broker, &context).await?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([vehicle_type.as_str()])
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?)
                .format(Representation::Normalized)
                .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let linked = broker.with_context(Context::Single(ctx.entry()?))?;
    let fragment = SubscriptionFragment::builder().watched_attributes(["speed"]).build()?;
    linked.subscriptions().update(&subscription_id, &fragment).send().await?;
    let subscription_2 = broker.with_accept(Accept::any()).subscriptions().retrieve(&subscription_id).send().await?;
    expect_that!(
        subscription_2.watched_attributes(),
        nth(0, eq(&format!("https://test.andromeda/{namespace}/speed").parse::<AttributeName>()?))
    );
    Ok(())
}
