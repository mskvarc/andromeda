//! A name the context maps to nothing is bad request data and keeps the stored one.

use andromeda_test_suite::{broker, config::SuiteConfig, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept},
    model::{
        Context,
        ContextEntry,
        ErrorType,
        InlineContext,
        NgsiLdMediaType,
        geometry::{Position, Shape},
    },
    query::geoquery,
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionFragment},
};

/// Pins: Update Subscription whose @context maps a watched attribute, a notified attribute or the geoQ geoproperty to nothing answers 400 `BadRequestData` and keeps the stored members
/// Clause: 5.5.7, 5.8.2.4, Table 5.5.2-1
#[gtest]
#[tokio::test]
async fn n_sub_upd_9012_a_name_the_context_maps_to_nothing_is_bad_request_data_and_keeps_the_stored_one() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let subscription_id = namespace.subscription_id(1)?;
    let term_probe_type = namespace.entity_type("TermProbe")?;
    let nothing_terms = broker
        .with_context(Context::Multiple(vec![ContextEntry::Inline(
            r#"{"speed": null, "Vehicle": null, "place": null}"#.parse::<InlineContext>()?,
        )]))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([term_probe_type.as_str()])
        .watched_attributes(["speed"])
        .notification(
            NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).build()?)
                .attributes(["speed"])
                .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let fragment = SubscriptionFragment::builder().watched_attributes(["speed"]).build()?;
    let answer = nothing_terms.with_responses().subscriptions().update(&subscription_id, &fragment).send().await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let fragment_2 = SubscriptionFragment::builder()
        .notification(
            NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).build()?)
                .attributes(["speed"])
                .build()?,
        )
        .build()?;
    let answer_2 = nothing_terms
        .with_responses()
        .subscriptions()
        .update(&subscription_id, &fragment_2)
        .send()
        .await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let fragment_3 = SubscriptionFragment::builder()
        .geo_q(
            geoquery::near_max(10, Shape::Point(Position::try_new(1.0, 2.0)?))
                .geoproperty("place")
                .build()?,
        )
        .build()?;
    let answer_3 = nothing_terms
        .with_responses()
        .subscriptions()
        .update(&subscription_id, &fragment_3)
        .send()
        .await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let stored = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await?;
    expect_that!(
        stored.watched_attributes().iter().map(ToString::to_string).collect::<Vec<_>>(),
        elements_are![eq("speed")]
    );
    expect_that!(
        stored.notification().attributes().iter().map(ToString::to_string).collect::<Vec<_>>(),
        elements_are![eq("speed")]
    );
    expect_that!(stored.geo_q(), none());
    broker.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
