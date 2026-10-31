//! An update is sent to the copy by its id reduced like a new copy.

use andromeda_test_suite::{
    body_json::{json_absent, json_equals},
    body_pattern::BodyPattern,
    broker,
    context_source::ContextSource,
    namespace::Namespace,
    notification_sink::NotificationSink,
    relayed_copy::{placed_copy_id, register_copy_source},
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::Method;
use ngsi_ld::{
    model::NgsiLdMediaType,
    query::{EntitySplitting, q},
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionFragment},
};

/// Pins: Update Subscription over a placed copy answers 204 and patches the copy at its own id; a split update
/// carries no q and no notification attributes
/// Clause: 5.8.1.4, 5.8.2.4
#[gtest]
#[tokio::test]
async fn n_sub_upd_6802_an_update_is_sent_to_the_copy_by_its_id_reduced_like_a_new_copy() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let source = ContextSource::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let subscription_id = namespace.subscription_id(1)?;
    register_copy_source(&broker, &source, &namespace, "s1").await?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([vehicle_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let copy_id = placed_copy_id(&source, "s1", &vehicle_type).await?;
    let fragment = SubscriptionFragment::builder()
        .description("marker-two")
        .q(q::attr("speed").gt(10).build()?)
        .split_entities(EntitySplitting::Split)
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?)
                .attributes(["speed"])
                .build()?,
        )
        .build()?;
    broker.subscriptions().update(&subscription_id, &fragment).send().await?;
    let patched = SourceRequest::new(Method::PATCH, &format!("/s1/ngsi-ld/v1/subscriptions/{copy_id}"))
        .body(BodyPattern::Selects("$[?(@.description == 'marker-two')]".parse()?));
    let forwarded = source.wait_for_matching(&patched, 1).await?;
    expect_that!(forwarded, len(eq(1)));
    // The forwarded fragment is read as the source received it: its absent members and its expanded type are the
    // bytes under test.
    let body = forwarded.first().or_fail()?.body.as_slice();
    expect_that!(body, json_absent("$.q"));
    expect_that!(body, json_absent("$.notification.attributes"));
    expect_that!(
        body,
        json_equals("$.entities[0].type", format!("https://uri.etsi.org/ngsi-ld/default-context/{vehicle_type}"))
    );
    Ok(())
}
