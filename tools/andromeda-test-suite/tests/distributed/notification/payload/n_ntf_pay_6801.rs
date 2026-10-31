//! A conformance of 1 0 removes observed at and reformats a language property.

use andromeda_test_suite::{
    attribute_matcher::{any_attribute, property},
    attribute_member::Member,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    notification_sink::NotificationSink,
    nth::nth,
    outcome::Done,
    problem::answered,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, operation::CreateSubscription},
    model::{Entity, LanguageMap, LanguageProperty, Lossless, NgsiLdMediaType, Property, RawJson},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: A notification of a subscription whose `ngsildConformance` is 1.0 carries speed without `observedAt` and
/// label, a `LanguageProperty`, as a Property
/// Clause: 5.8.6, 4.3.6.8
#[gtest]
#[tokio::test]
async fn n_ntf_pay_6801_a_conformance_of_1_0_removes_observed_at_and_reformats_a_language_property() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([vehicle_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    // The model refuses it (a subscription ngsildConformance), so `ngsildConformance` is written as text.
    let ngsild_conformance: RawJson = r#""1.0""#.parse()?;
    let document = UncheckedDocument::try_from(subscription.render())?.set_member("ngsildConformance", &ngsild_conformance);
    let request = broker.unchecked(&CreateSubscription { subscription: &subscription })?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let vehicle = Entity::builder()
        .id(vehicle_id)
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(20).observed_at("2025-01-01T00:00:00Z".parse()?))
        .language_property(
            "label",
            LanguageProperty::builder(LanguageMap::try_from_pairs([("en", "car"), ("es", "coche")])?),
        )
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let notifications = sink.wait_for_at_least(1, 1).await?;
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(
        notification.data().as_slice(),
        nth(
            0,
            all!(
                has_attribute("speed", any_attribute().value(20).without(Member::ObservedAt)),
                has_attribute("label", property())
            )
        )
    );
    Ok(())
}
