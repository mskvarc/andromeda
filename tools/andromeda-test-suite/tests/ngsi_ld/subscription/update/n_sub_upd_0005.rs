//! A link header does not replace the stored jsonld context.

use andromeda_test_suite::{broker, config::SuiteConfig, hosted_context::HostedContext, namespace::Namespace, notification_sink::NotificationSink};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Context, ContextEntry, InlineContext, NgsiLdMediaType},
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionFragment},
};

/// Pins: an update sent under a Link header naming another @context reads the fragment with that @context but keeps
/// the stored `jsonldContext`, which only the create initialises
/// Clause: 5.8.2.4, 5.8.1.4, 5.5.8, 5.2.12
#[gtest]
#[tokio::test]
async fn n_sub_upd_0005_a_link_header_does_not_replace_the_stored_jsonld_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let config = SuiteConfig::get()?;
    let subscription_id = namespace.subscription_id(1)?;
    let building_type = namespace.entity_type("Building")?;
    let context = Context::Single(ContextEntry::Inline(
        format!(r#"{{"Probe": "https://test.andromeda/{namespace}/Probe"}}"#).parse::<InlineContext>()?,
    ));
    let other_context = HostedContext::add(&broker, &context).await?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([building_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .jsonld_context(config.core_context().clone())
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let linked = broker.with_context(Context::Single(other_context.entry()?))?;
    let fragment = SubscriptionFragment::builder()
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(2)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    linked.subscriptions().update(&subscription_id, &fragment).send().await?;
    let subscription_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await?;
    expect_that!(subscription_2.notification().endpoint().uri(), eq(&sink.endpoint(2)?));
    expect_that!(subscription_2.jsonld_context(), some(eq(config.core_context())));
    Ok(())
}
