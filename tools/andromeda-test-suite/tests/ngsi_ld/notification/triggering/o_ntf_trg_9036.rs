//! A scope change raises no attribute trigger.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, notification_sink::NotificationSink, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, EntityFragment, Lossless, NgsiLdMediaType, OneOrMany, Representation, Scope, ScopeChange},
    subscription::{Endpoint, NotificationParams, NotificationTrigger, Subscription},
};
use std::time::Duration;

/// How long each endpoint is watched for a notification beyond the ones owed.
const QUIET_WINDOW: Duration = Duration::from_secs(3);

/// Pins: a Subscription with the default triggers and one with `entityUpdated` are not notified of the creation of a
/// Vehicle that carries only `scope`, nor of a Merge Entity that adds the scope of a Vehicle, one that changes it and
/// one that removes it; each is notified of that Vehicle's creation with `speed`, which raises `attributeCreated`, and
/// of a merge changing `speed`, and of nothing else
/// Clause: 4.18, 5.2.12, 5.6.17, 5.8.6
/// Recorded: no clause says whether `scope` raises an attribute trigger. Clause 4.18 represents it as "a special
/// scope Property", while Table 5.2.12-1 gives `watchedAttributes` "Watched Attributes (Properties or
/// Relationships)" and Table 5.2.14.1-1 names `pick` and `omit` members "id, type, scope or a projected Attribute
/// name"; Andromeda reads `scope` as an Entity member that is no Attribute, so it raises no `attributeCreated`,
/// `attributeUpdated` or `attributeDeleted`, and none of `entityUpdated`, which Table 5.2.12-1 makes their
/// combination
#[gtest]
#[tokio::test]
async fn o_ntf_trg_9036_a_scope_change_raises_no_attribute_trigger() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let default_id = namespace.subscription_id("default-triggers")?;
    let updated_id = namespace.subscription_id("entity-updated")?;
    for (id, endpoint, triggers) in [
        (&default_id, "default", None),
        (&updated_id, "updated", Some([NotificationTrigger::EntityUpdated])),
    ] {
        let builder = Subscription::builder().id(id.clone()).entities([vehicle_type.as_str()]).notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(endpoint)?).accept(NgsiLdMediaType::Json).build()?)
                .format(Representation::Normalized)
                .build()?,
        );
        let subscription = match triggers {
            Some(triggers) => builder.notification_triggers(triggers).build()?,
            None => builder.build()?,
        };
        broker.subscriptions().create(&subscription).send().await?;
    }
    let scoped_only = Entity::builder()
        .id(namespace.urn("Vehicle", "scoped-only")?)
        .entity_type(vehicle_type.clone())
        .scope(format!("/{namespace}/A").as_str())
        .build()?;
    broker.entities().create(&scoped_only).send().await?.done()?;
    let moving_id = namespace.urn("Vehicle", "moving")?;
    let moving = Entity::builder()
        .id(moving_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 1)
        .build()?;
    broker.entities().create(&moving).send().await?.done()?;
    for change in [
        ScopeChange::Set(OneOrMany::one(format!("/{namespace}/A").parse::<Scope>()?)),
        ScopeChange::Set(OneOrMany::one(format!("/{namespace}/B").parse::<Scope>()?)),
        ScopeChange::Delete,
    ] {
        let fragment = EntityFragment::builder().scope(change).build()?;
        broker.entities().merge(&moving_id, &fragment).send().await?.done()?;
    }
    let speed_change = EntityFragment::builder().property("speed", 2).build()?;
    broker.entities().merge(&moving_id, &speed_change).send().await?.done()?;
    for endpoint in ["default", "updated"] {
        sink.wait_for_at_least(endpoint, 2).await?;
        expect_that!(sink.count_after(endpoint, QUIET_WINDOW).await?, eq(2), "{endpoint}");
        for received in sink.received(endpoint).await? {
            expect_that!(
                received.entities(Lossless::Normalized)?.data().as_slice(),
                elements_are![has_id(&moving_id)],
                "{endpoint}"
            );
        }
    }
    broker.subscriptions().delete(&default_id).send().await?;
    broker.subscriptions().delete(&updated_id).send().await?;
    Ok(())
}
