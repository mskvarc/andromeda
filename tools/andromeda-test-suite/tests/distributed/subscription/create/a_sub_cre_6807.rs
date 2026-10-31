//! A subscription not naming split entities places a copy keeping q and dropping the attribute projection.

use andromeda_test_suite::{
    body_json::{json_absent, json_exists},
    broker,
    context_source::ContextSource,
    namespace::Namespace,
    notification_sink::NotificationSink,
    relayed_copy::{placed_copy, register_copy_source},
};
use googletest::prelude::*;
use ngsi_ld::{
    model::NgsiLdMediaType,
    query::q,
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Create Subscription without `splitEntities`, with a q and notification attributes, places a copy that
/// keeps the q and carries no notification attributes
/// Clause: 5.8.1.4
/// Recorded: 5.8.1.4 lets the deployment default decide whether entities may be split, and Andromeda defaults to
/// complete entities, so filters stay with the copy
#[gtest]
#[tokio::test]
async fn a_sub_cre_6807_a_subscription_not_naming_split_entities_places_a_copy_keeping_q_and_dropping_the_attribute_projection() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let source = ContextSource::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    register_copy_source(&broker, &source, &namespace, "s1").await?;
    let subscription = Subscription::builder()
        .id(namespace.subscription_id(1)?)
        .entities([vehicle_type.as_str()])
        .q(q::attr("speed").gt(10).build()?)
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?)
                .attributes(["speed"])
                .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let copy = placed_copy(&source, "s1", &vehicle_type).await?;
    // The typed reader refuses the q of the copy, so the body is read for the two members the clause pins.
    expect_that!(copy.body.as_slice(), json_exists("$.q"));
    expect_that!(copy.body.as_slice(), json_absent("$.notification.attributes"));
    Ok(())
}
