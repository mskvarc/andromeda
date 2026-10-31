//! A change followed by a burst of changes is still notified.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    config::SuiteConfig,
    identity_matcher::has_id,
    namespace::Namespace,
    notification_sink::NotificationSink,
    nth::nth,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, EntityFragment, Input, Lossless, NgsiLdMediaType},
    query::{EntitySelector, Join, builders::SelectorTypeBuilder},
    registration::{CSourceRegistration, RegistrationInfo},
    subscription::{Endpoint, NotificationParams, Subscription},
};
use std::iter;

/// Pins: A Subscription to a type is notified of the creation of an Entity of that type when, right after it, 9,000
/// Entities of another type are created while the tenant is busy evaluating six changes against a Subscription with
/// a 2,000-level inline join
/// Clause: 5.8.6, 5.8.1.4
/// Tenant: private (the file overflows its tenant's live change buffer, which in the shared tenant would drop the
/// notifications of every other file)
#[gtest]
#[tokio::test]
async fn n_ntf_dlv_9001_a_change_followed_by_a_burst_of_changes_is_still_notified() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let config = SuiteConfig::get()?;
    let tenant = broker.with_tenant(namespace.tenant()?);
    let registration_provision_id = namespace.registration_id("provision")?;
    let sentinel_type = namespace.entity_type("Sentinel")?;
    let building_0_id = namespace.urn("Building", "0")?;
    let building_type = namespace.entity_type("Building")?;
    let building_9_id = namespace.urn("Building", 9)?;
    let building_10_id = namespace.urn("Building", 10)?;
    let building_99_id = namespace.urn("Building", 99)?;
    let building_100_id = namespace.urn("Building", 100)?;
    let building_999_id = namespace.urn("Building", 999)?;
    let building_1000_id = namespace.urn("Building", 1000)?;
    let building_2000_id = namespace.urn("Building", 2000)?;
    let subscription_slow_id = namespace.subscription_id("slow")?;
    let subscription_victim_id = namespace.subscription_id("victim")?;
    let victim_type = namespace.entity_type("Victim")?;
    let victim_id = namespace.urn("Victim", 1)?;
    let filler_type = namespace.entity_type("Filler")?;
    let registration_provision = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_provision_id.clone())
        .information([RegistrationInfo::builder().entities([sentinel_type.as_str()]).build()?])
        .build()?;
    tenant.registrations().register(&registration_provision).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_provision_cleanup = Cleanup::of(&tenant).registration(&registration_provision_id);
    tenant.registrations().delete(&registration_provision_id).send().await?;
    let entities = (0..=8)
        .map(|n| -> Result<Entity<Input>> {
            Ok(Entity::builder()
                .id(namespace.urn("Building", n)?)
                .entity_type(building_type.clone())
                .relationship("linkedTo", namespace.urn("Building", n + 1)?)
                .build()?)
        })
        .chain(iter::once(Ok(Entity::builder()
            .id(building_9_id)
            .entity_type(building_type.clone())
            .relationship("linkedTo", building_10_id)
            .build()?)))
        .chain((10..=98).map(|n| -> Result<Entity<Input>> {
            Ok(Entity::builder()
                .id(namespace.urn("Building", format!("{n:02}"))?)
                .entity_type(building_type.clone())
                .relationship("linkedTo", namespace.urn("Building", format!("{:02}", n + 1))?)
                .build()?)
        }))
        .chain(iter::once(Ok(Entity::builder()
            .id(building_99_id)
            .entity_type(building_type.clone())
            .relationship("linkedTo", building_100_id)
            .build()?)))
        .chain((100..=499).map(|n| -> Result<Entity<Input>> {
            Ok(Entity::builder()
                .id(namespace.urn("Building", format!("{n:03}"))?)
                .entity_type(building_type.clone())
                .relationship("linkedTo", namespace.urn("Building", format!("{:03}", n + 1))?)
                .build()?)
        }))
        .collect::<Result<Vec<_>>>()?;
    tenant.entities().batch_create(&entities).send().await?.done()?;
    let entities_2 = (500..=998)
        .map(|n| -> Result<Entity<Input>> {
            Ok(Entity::builder()
                .id(namespace.urn("Building", format!("{n:03}"))?)
                .entity_type(building_type.clone())
                .relationship("linkedTo", namespace.urn("Building", format!("{:03}", n + 1))?)
                .build()?)
        })
        .chain(iter::once(Ok(Entity::builder()
            .id(building_999_id)
            .entity_type(building_type.clone())
            .relationship("linkedTo", building_1000_id)
            .build()?)))
        .collect::<Result<Vec<_>>>()?;
    tenant.entities().batch_create(&entities_2).send().await?.done()?;
    let entities_3 = (1000..=1499)
        .map(|n| -> Result<Entity<Input>> {
            Ok(Entity::builder()
                .id(namespace.urn("Building", format!("{n:04}"))?)
                .entity_type(building_type.clone())
                .relationship("linkedTo", namespace.urn("Building", format!("{:04}", n + 1))?)
                .build()?)
        })
        .collect::<Result<Vec<_>>>()?;
    tenant.entities().batch_create(&entities_3).send().await?.done()?;
    let entities_4 = (1500..=1999)
        .map(|n| -> Result<Entity<Input>> {
            Ok(Entity::builder()
                .id(namespace.urn("Building", format!("{n:04}"))?)
                .entity_type(building_type.clone())
                .relationship("linkedTo", namespace.urn("Building", format!("{:04}", n + 1))?)
                .build()?)
        })
        .collect::<Result<Vec<_>>>()?;
    tenant.entities().batch_create(&entities_4).send().await?.done()?;
    let entities_5 = [Entity::builder().id(building_2000_id).entity_type(building_type.clone()).build()?];
    tenant.entities().batch_create(&entities_5).send().await?.done()?;
    let subscription_slow = Subscription::builder()
        .id(subscription_slow_id.clone())
        .entities([EntitySelector::new(SelectorTypeBuilder::from(&building_type).build()?).with_ids(building_0_id.clone())])
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint("slow")?).accept(NgsiLdMediaType::Json).build()?)
                .join(Join::Inline)
                .join_level(2000)
                .build()?,
        )
        .build()?;
    tenant.subscriptions().create(&subscription_slow).send().await?;
    let subscription_victim = Subscription::builder()
        .id(subscription_victim_id.clone())
        .entities([victim_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("victim")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    tenant.subscriptions().create(&subscription_victim).send().await?;
    // Each change to the head is evaluated against the slow Subscription by inlining 2,000 Linked Entities, so the six of them keep the notification evaluator of the tenant busy for several seconds.
    let fragment = EntityFragment::builder().property("name", "touch-0").build()?;
    tenant.entities().append_attributes(&building_0_id, &fragment).send().await?.done()?;
    let fragment_2 = EntityFragment::builder().property("name", "touch-1").build()?;
    tenant.entities().append_attributes(&building_0_id, &fragment_2).send().await?.done()?;
    let fragment_3 = EntityFragment::builder().property("name", "touch-2").build()?;
    tenant.entities().append_attributes(&building_0_id, &fragment_3).send().await?.done()?;
    let fragment_4 = EntityFragment::builder().property("name", "touch-3").build()?;
    tenant.entities().append_attributes(&building_0_id, &fragment_4).send().await?.done()?;
    let fragment_5 = EntityFragment::builder().property("name", "touch-4").build()?;
    tenant.entities().append_attributes(&building_0_id, &fragment_5).send().await?.done()?;
    let fragment_6 = EntityFragment::builder().property("name", "touch-5").build()?;
    tenant.entities().append_attributes(&building_0_id, &fragment_6).send().await?.done()?;
    // The change under test, which the victim Subscription is owed a notification for.
    let victim = Entity::builder()
        .id(victim_id.clone())
        .entity_type(victim_type.clone())
        .property("speed", 1)
        .build()?;
    tenant.entities().create(&victim).send().await?.done()?;
    // 9,000 further changes, more than the 8,192 the live change buffer holds, arrive while the evaluator is still busy.
    let entities_6 = (0..=999)
        .map(|n| -> Result<Entity<Input>> {
            Ok(Entity::builder()
                .id(namespace.urn("Filler", format!("{n:04}"))?)
                .entity_type(filler_type.clone())
                .build()?)
        })
        .collect::<Result<Vec<_>>>()?;
    tenant.entities().batch_create(&entities_6).send().await?.done()?;
    let entities_7 = (1000..=1999)
        .map(|n| -> Result<Entity<Input>> {
            Ok(Entity::builder()
                .id(namespace.urn("Filler", format!("{n:04}"))?)
                .entity_type(filler_type.clone())
                .build()?)
        })
        .collect::<Result<Vec<_>>>()?;
    tenant.entities().batch_create(&entities_7).send().await?.done()?;
    let entities_8 = (2000..=2999)
        .map(|n| -> Result<Entity<Input>> {
            Ok(Entity::builder()
                .id(namespace.urn("Filler", format!("{n:04}"))?)
                .entity_type(filler_type.clone())
                .build()?)
        })
        .collect::<Result<Vec<_>>>()?;
    tenant.entities().batch_create(&entities_8).send().await?.done()?;
    let entities_9 = (3000..=3999)
        .map(|n| -> Result<Entity<Input>> {
            Ok(Entity::builder()
                .id(namespace.urn("Filler", format!("{n:04}"))?)
                .entity_type(filler_type.clone())
                .build()?)
        })
        .collect::<Result<Vec<_>>>()?;
    tenant.entities().batch_create(&entities_9).send().await?.done()?;
    let entities_10 = (4000..=4999)
        .map(|n| -> Result<Entity<Input>> {
            Ok(Entity::builder()
                .id(namespace.urn("Filler", format!("{n:04}"))?)
                .entity_type(filler_type.clone())
                .build()?)
        })
        .collect::<Result<Vec<_>>>()?;
    tenant.entities().batch_create(&entities_10).send().await?.done()?;
    let entities_11 = (5000..=5999)
        .map(|n| -> Result<Entity<Input>> {
            Ok(Entity::builder()
                .id(namespace.urn("Filler", format!("{n:04}"))?)
                .entity_type(filler_type.clone())
                .build()?)
        })
        .collect::<Result<Vec<_>>>()?;
    tenant.entities().batch_create(&entities_11).send().await?.done()?;
    let entities_12 = (6000..=6999)
        .map(|n| -> Result<Entity<Input>> {
            Ok(Entity::builder()
                .id(namespace.urn("Filler", format!("{n:04}"))?)
                .entity_type(filler_type.clone())
                .build()?)
        })
        .collect::<Result<Vec<_>>>()?;
    tenant.entities().batch_create(&entities_12).send().await?.done()?;
    let entities_13 = (7000..=7999)
        .map(|n| -> Result<Entity<Input>> {
            Ok(Entity::builder()
                .id(namespace.urn("Filler", format!("{n:04}"))?)
                .entity_type(filler_type.clone())
                .build()?)
        })
        .collect::<Result<Vec<_>>>()?;
    tenant.entities().batch_create(&entities_13).send().await?.done()?;
    let entities_14 = (8000..=8999)
        .map(|n| -> Result<Entity<Input>> {
            Ok(Entity::builder()
                .id(namespace.urn("Filler", format!("{n:04}"))?)
                .entity_type(filler_type.clone())
                .build()?)
        })
        .collect::<Result<Vec<_>>>()?;
    tenant.entities().batch_create(&entities_14).send().await?.done()?;
    sink.wait_for_at_least("victim", 1).await?;
    let notifications = sink.received("victim").await?;
    expect_that!(notifications, len(eq(1)));
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification.data().as_slice(), nth(0, has_id(&victim_id)));
    tenant.subscriptions().delete(&subscription_slow_id).send().await?;
    tenant.subscriptions().delete(&subscription_victim_id).send().await?;
    Ok(())
}
