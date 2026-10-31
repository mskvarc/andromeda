//! Watched attributes are stored as the iris their terms expand to.

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
    model::{AttributeName, Context, ContextEntry, InlineContext, NgsiLdMediaType},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Create Subscription under a context mapping `speed_ns` to a vocabulary IRI stores `watchedAttributes`
/// expanded: Retrieve Subscription under the core context alone reads the IRI, under the creating context the term
/// Clause: 5.8.1.4, 5.5.7
#[gtest]
#[tokio::test]
async fn n_sub_cre_6803_watched_attributes_are_stored_as_the_iris_their_terms_expand_to() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let context = Context::Multiple(vec![
        ContextEntry::Inline(format!(r#"{{"speed_{namespace}": "https://test.andromeda/{namespace}/speed"}}"#).parse::<InlineContext>()?),
        core_entry()?,
    ]);
    let ctx = HostedContext::add(&broker, &context).await?;
    let linked = broker.with_context(Context::Single(ctx.entry()?))?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([vehicle_type.as_str()])
        .watched_attributes([namespace.attribute("speed")?.to_string()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    linked.subscriptions().create(&subscription).send().await?;
    let subscription_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await?;
    expect_that!(
        subscription_2.watched_attributes(),
        nth(0, eq(&format!("https://test.andromeda/{namespace}/speed").parse::<AttributeName>()?))
    );
    let subscription_3 = linked
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await?;
    expect_that!(
        subscription_3.watched_attributes(),
        nth(0, eq(&format!("speed_{namespace}").parse::<AttributeName>()?))
    );
    Ok(())
}
