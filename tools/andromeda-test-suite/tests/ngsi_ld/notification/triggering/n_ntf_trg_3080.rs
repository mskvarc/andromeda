//! Watched attributes naming a full iri trigger under a user context.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    hosted_context::HostedContext,
    identity_matcher::has_id,
    namespace::Namespace,
    notification_sink::NotificationSink,
    nth::nth,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Attribute, AttributeName, Context, ContextEntry, Entity, InlineContext, Lossless, NgsiLdMediaType, Property, Representation},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: a subscription created under a Link header carrying a user context whose `watchedAttributes` names an
/// attribute by its full IRI notifies when that attribute changes and not when another does
/// Clause: 5.5.7, 5.8.6, 5.2.12
#[gtest]
#[tokio::test]
async fn n_ntf_trg_3080_watched_attributes_naming_a_full_iri_trigger_under_a_user_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let subscription_id = namespace.subscription_id(1)?;
    let context = Context::Single(ContextEntry::Inline(format!(r#"{{"Building_{namespace}": "https://test.andromeda/{namespace}/Building", "subCategory": "https://test.andromeda/{namespace}/subCategory", "airQualityLevel": "https://test.andromeda/{namespace}/airQualityLevel"}}"#).parse::<InlineContext>()?));
    let ctx = HostedContext::add(&broker, &context).await?;
    let linked = broker.with_context(Context::Single(ctx.entry()?))?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .property("subCategory", "tourism")
        .property("airQualityLevel", 4)
        .build()?;
    linked.entities().create(&building).send().await?.done()?;
    // 5.5.7: an absolute IRI expands to itself
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([building_type.as_str()])
        .watched_attributes([format!("https://test.andromeda/{namespace}/subCategory").as_str()])
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?)
                .format(Representation::Normalized)
                .build()?,
        )
        .build()?;
    linked.subscriptions().create(&subscription).send().await?;
    // the unwatched attribute changes first: it must notify nothing, so the oldest notification is the watched change's
    let patch = Attribute::from(Property::builder(9).build()?).into_patch();
    linked
        .entities()
        .update_attribute(&building_id, &"airQualityLevel".parse::<AttributeName>()?, patch)
        .send()
        .await?
        .done()?;
    let patch_2 = Attribute::from(Property::builder("culture").build()?).into_patch();
    linked
        .entities()
        .update_attribute(&building_id, &"subCategory".parse::<AttributeName>()?, patch_2)
        .send()
        .await?
        .done()?;
    sink.wait_for_at_least(1, 1).await?;
    let notifications = sink.received(1).await?;
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(
        notification.data().as_slice(),
        nth(0, all!(has_id(&building_id), has_attribute("subCategory", any_attribute().value("culture"))))
    );
    Ok(())
}
