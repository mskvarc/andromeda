//! A split subscription places a copy without q scope q geo q and the notification attribute projection.

use andromeda_test_suite::{
    broker,
    context_source::ContextSource,
    forwarded_body,
    namespace::Namespace,
    notification_sink::NotificationSink,
    nth::nth,
    relayed_copy::{placed_copy, register_copy_source},
};
use googletest::prelude::*;
use ngsi_ld::{
    model::{
        NgsiLdMediaType,
        geometry::{Position, Shape},
    },
    query::{EntitySelector, EntitySplitting, SelectorType, geoquery, q, scope_query::ScopeQuery, type_selection},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Create Subscription with `splitEntities` true, a q, a `scopeQ`, a `geoQ` and notification attributes
/// places a copy that carries none of them
/// Clause: 5.8.1.4
#[gtest]
#[tokio::test]
async fn n_sub_cre_6806_a_split_subscription_places_a_copy_without_q_scope_q_geo_q_and_the_notification_attribute_projection() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let source = ContextSource::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    register_copy_source(&broker, &source, &namespace, "s1").await?;
    let subscription = Subscription::builder()
        .id(namespace.subscription_id(1)?)
        .entities([vehicle_type.as_str()])
        .split_entities(EntitySplitting::Split)
        .q(q::attr("speed").gt(10).build()?)
        .scope_q("/Madrid".parse::<ScopeQuery>()?)
        .geo_q(geoquery::near_max(2000, Shape::Point(Position::try_new(0.0, 0.0)?)).build()?)
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?)
                .attributes(["speed"])
                .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let copy = forwarded_body::subscription(&placed_copy(&source, "s1", &vehicle_type).await?)?;
    expect_that!(
        copy.entities().iter().map(EntitySelector::selector_type).collect::<Vec<_>>(),
        nth(
            0,
            eq(&&SelectorType::Selection(
                type_selection::entity_type(format!("https://uri.etsi.org/ngsi-ld/default-context/{vehicle_type}").as_str()).build()?
            ))
        )
    );
    expect_that!(copy.q(), none());
    expect_that!(copy.scope_q(), none());
    expect_that!(copy.geo_q(), none());
    expect_that!(copy.notification().attributes(), is_empty());
    Ok(())
}
