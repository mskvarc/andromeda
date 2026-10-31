//! A notified ordinate is its nearest double.

use andromeda_test_suite::{body_pattern::BodyPattern, broker, namespace::Namespace, notification_sink::NotificationSink, problem::answered};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    model::{
        Entity,
        EntityId,
        NgsiLdMediaType,
        Value,
        geometry::{Position, Shape},
    },
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Creating a Tree whose location is the Point `[13.123456789012345678, 52.5]` and whose girth Property is
/// 13.123456789012345678 notifies the ordinate as 13.123456789012346 and the girth as 13.123456789012345678
/// Clause: 4.5.2.2, 4.7.2, 5.3.1
/// Recorded: clause 4.7.2 leaves the precision of a position to the implementation; Andromeda holds every ordinate
/// as the double nearest to it and writes it in the shortest spelling of that double, while a Property number keeps
/// its exact decimal value
#[gtest]
#[tokio::test]
async fn o_ntf_pay_9040_a_notified_ordinate_is_its_nearest_double() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let tree_type = namespace.entity_type("Tree")?;
    let tree_id = format!("urn:ngsi-ld:Tree:{namespace}");
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([tree_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let tree = Entity::builder()
        .id(tree_id.parse::<EntityId>()?)
        .entity_type(tree_type)
        .geo_property(
            "location",
            Shape::Point(Position::try_from_numbers("13.123456789012345678".parse()?, "52.5".parse()?)?),
        )
        .property("girth", Value::Number("13.123456789012345678".parse()?))
        .build()?;
    let created = broker.with_responses().entities().create(&tree).send().await;
    expect_that!(&created, answered(StatusCode::CREATED));
    let spelled = [
        BodyPattern::Contains("\"coordinates\":[13.123456789012346,52.5]".to_owned()),
        BodyPattern::Contains("\"value\":13.123456789012345678".to_owned()),
    ];
    sink.wait_for_at_least_where(1, &spelled, 1).await?;
    expect_that!(sink.received(1).await?, len(eq(1)));
    broker.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
