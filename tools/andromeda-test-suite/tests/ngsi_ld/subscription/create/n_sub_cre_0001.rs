//! Accepts a q with a linked entity path and keeps it.

use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::{Accept, Location},
    model::NgsiLdMediaType,
    query::q,
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: a q holding a Linked Entity relation (`AttrName{AttrName}` followed by a comparison) is a valid query, so
/// the subscription is created and reads back with the q as written
/// Clause: 5.2.12, 4.9, 5.8.1.4
#[gtest]
#[tokio::test]
async fn n_sub_cre_0001_accepts_a_q_with_a_linked_entity_path_and_keeps_it() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([vehicle_type.as_str()])
        .q(q::attr("isParkedIn").linked("category").eq("commercial").build()?)
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    let received = broker.with_responses().subscriptions().create(&subscription).send().await?;
    expect_that!(
        received.head.typed::<Location>().valid().map(|location| location.as_iri_ref().as_str()),
        some(ends_with(format!("/ngsi-ld/v1/subscriptions/urn:ngsi-ld:Subscription:{namespace}-1")))
    );
    let subscription_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await?;
    expect_that!(subscription_2.id(), eq(&subscription_id));
    expect_that!(subscription_2.q().map(ToString::to_string), some(eq("isParkedIn{category}==\"commercial\"")));
    Ok(())
}
