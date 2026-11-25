//! A quoted prefixed name selects alike in a Subscription and in Query Entities.

use andromeda_test_suite::{
    broker,
    hosted_context::HostedContext,
    identity_matcher::{has_id, ids_in_order},
    namespace::Namespace,
    notification_sink::NotificationSink,
    nth::nth,
    outcome::Done,
    problem::answered,
    quoted_name_query::{context_link, query_by_text},
    unchecked_page::entities,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, operation::CreateSubscription},
    model::{Context, ContextEntry, Entity, InlineContext, Lossless, NgsiLdMediaType},
    subscription::{Endpoint, NotificationParams, NotificationTrigger, Subscription},
};
use std::slice;

/// Pins: a Subscription with q="ex:speed">50, created under an @context holding the prefix ex, notifies for the
/// creation of the Car whose ex:speed is 80 and not for the Car whose ex:speed is 20, and Query Entities with the same
/// q under the same @context returns the same Car
/// Clause: 4.9, 4.6.2, 5.8.1, 5.8.6
#[gtest]
#[tokio::test]
async fn a_qry_qlg_9073_a_quoted_prefixed_name_selects_alike_in_a_subscription_and_a_query() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let prefixed = HostedContext::add(
        &broker,
        &Context::Single(ContextEntry::Inline(
            format!(r#"{{"ex":"https://example.org/{namespace}/"}}"#).parse::<InlineContext>()?,
        )),
    )
    .await?;
    let linked = broker.with_context(Context::Single(prefixed.entry()?))?;
    let car_type = namespace.entity_type("Car")?;
    let fast_id = namespace.urn("Car", "fast")?;
    let subscription_id = namespace.subscription_id(1)?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([car_type.as_str()])
        .notification_triggers([NotificationTrigger::EntityCreated])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    // The q model writes a name only as an unquoted AttrName, so `q` is written as text.
    let document = UncheckedDocument::try_from(subscription.render())?.set_text("q", r#""ex:speed">50"#);
    let request = linked.unchecked(&CreateSubscription { subscription: &subscription })?.document(&document);
    let created = linked.send_unchecked(request).await;
    expect_that!(&created, answered(StatusCode::CREATED));
    let slow = Entity::builder()
        .id(namespace.urn("Car", "slow")?)
        .entity_type(car_type.clone())
        .property("ex:speed", 20)
        .build()?;
    linked.entities().create(&slow).send().await?.done()?;
    let fast = Entity::builder()
        .id(fast_id.clone())
        .entity_type(car_type.clone())
        .property("ex:speed", 80)
        .build()?;
    linked.entities().create(&fast).send().await?.done()?;
    // One notification, of the second creation: the slow Car was created first and sent none.
    sink.wait_for_at_least(1, 1).await?;
    let notifications = sink.received(1).await?;
    expect_that!(notifications, len(eq(1)));
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification.subscription_id(), eq(&subscription_id));
    expect_that!(notification.data().as_slice(), len(eq(1)));
    expect_that!(notification.data().as_slice(), nth(0, has_id(&fast_id)));
    let answer = query_by_text(&broker, &car_type, r#""ex:speed">50"#, &[], Some(&context_link(&prefixed)?)).await?;
    expect_that!(answer.status(), eq(StatusCode::OK));
    expect_that!(entities(&answer)?.as_slice(), ids_in_order(slice::from_ref(&fast_id)));
    Ok(())
}
