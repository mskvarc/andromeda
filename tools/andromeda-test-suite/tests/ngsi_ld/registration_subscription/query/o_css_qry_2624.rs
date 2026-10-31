//! Query csource subscriptions answers one link line with the context link first.

use andromeda_test_suite::{
    broker,
    link_matcher::{link_value, relations_in_order},
    namespace::Namespace,
    notification_sink::NotificationSink,
    wire::header_values,
};
use googletest::prelude::*;
use http::header::LINK;
use ngsi_ld::{
    http::headers::{Accept, Link, RelationType},
    model::NgsiLdMediaType,
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Query Context Source Registration Subscriptions answering application/json over several pages carries one
/// Link field line holding the @context link first, then the pagination links
/// Clause: 6.3.6, 6.3.10, 5.11.5
/// Recorded: Andromeda answers the Link field on one line; RFC 9110 clause 5.3 makes one combined line and several
/// lines equivalent, and the test pins the single line
#[gtest]
#[tokio::test]
async fn o_css_qry_2624_query_csource_subscriptions_answers_one_link_line_with_the_context_link_first() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let building_type = namespace.entity_type("Building")?;
    let subscription_2_id = namespace.subscription_id(2)?;
    let subscription = Subscription::registration_builder()
        .id(subscription_id.clone())
        .entities([building_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("c1")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.registration_subscriptions().create(&subscription).send().await?;
    let subscription_2 = Subscription::registration_builder()
        .id(subscription_2_id)
        .entities([building_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("c2")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.registration_subscriptions().create(&subscription_2).send().await?;
    let headed = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registration_subscriptions()
        .query()
        .limit(1)
        .offset(0)
        .send()
        .await?;
    let page = headed.outcome;
    expect_that!(header_values(headed.head.headers(), &LINK), len(eq(1)));
    expect_that!(
        headed.head.typed::<Link>().valid().and_then(|link| link.values().first()),
        some(link_value(RelationType::JsonLdContext).with_media_type("application/ld+json"))
    );
    expect_that!(
        headed.head.typed::<Link>().valid(),
        some(relations_in_order([RelationType::JsonLdContext, RelationType::Next]))
    );
    expect_that!(page.items(), len(eq(1)));
    Ok(())
}
