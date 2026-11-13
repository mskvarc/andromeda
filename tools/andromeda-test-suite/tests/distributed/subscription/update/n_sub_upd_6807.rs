//! An update reaches the copy narrowed to the registration.

use andromeda_test_suite::{
    body_pattern::BodyPattern,
    broker,
    context_source::ContextSource,
    forwarded_body,
    namespace::Namespace,
    notification_sink::NotificationSink,
    relayed_copy::{copy_selector, placed_copy_id, register_copy_source_of},
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::Method;
use ngsi_ld::{
    model::{MemberChange, NgsiLdMediaType},
    query::{EntitySelector, builders::SelectorTypeBuilder, type_selection},
    registration::EntityInfo,
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionFragment},
};

/// Pins: Update Subscription replacing the selectors with `Vehicle|Bus` over a `federationOps` registration of one
/// `Vehicle` sends the copy a fragment whose only selector names `Vehicle` and the registered `id`
/// Clause: 5.8.1.4, 5.8.2.4, 4.3.6.1
#[gtest]
#[tokio::test]
async fn n_sub_upd_6807_an_update_reaches_the_copy_narrowed_to_the_registration() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let source = ContextSource::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let bus_type = namespace.entity_type("Bus")?;
    let vehicle_id = namespace.urn("Vehicle", 7)?;
    let subscription_id = namespace.subscription_id(1)?;
    let registered = EntityInfo::new(vehicle_type.clone()).with_id(vehicle_id.clone());
    register_copy_source_of(&broker, &source, &namespace, "s1", vec![registered]).await?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([vehicle_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let copy_id = placed_copy_id(&source, "s1", &vehicle_type).await?;
    let either = SelectorTypeBuilder::from(type_selection::entity_type(vehicle_type.as_str()).or(bus_type.as_str())).build()?;
    let fragment = SubscriptionFragment::builder()
        .description("narrowed")
        .entities([EntitySelector::new(either)])
        .build()?;
    broker.subscriptions().update(&subscription_id, &fragment).send().await?;
    let patched = SourceRequest::new(Method::PATCH, &format!("/s1/ngsi-ld/v1/subscriptions/{copy_id}"))
        .body(BodyPattern::Selects("$[?(@.description == 'narrowed')]".parse()?));
    let forwarded = source.wait_for_matching(&patched, 1).await?;
    expect_that!(forwarded, len(eq(1)));
    let sent = forwarded_body::subscription_fragment(forwarded.first().or_fail()?)?;
    let expected = [copy_selector(&vehicle_type)?.with_ids(vehicle_id)];
    expect_that!(sent.entities(), some(eq(MemberChange::Set(expected.as_slice()))));
    Ok(())
}
