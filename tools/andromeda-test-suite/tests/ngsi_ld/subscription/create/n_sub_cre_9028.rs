//! A name the context maps to nothing is bad request data.

use andromeda_test_suite::{
    broker,
    config::SuiteConfig,
    namespace::Namespace,
    problem::{answered, problem},
};
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
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Create Subscription whose @context maps an entity type, a watched attribute, a notified attribute or the geoQ geoproperty to nothing answers 400 `BadRequestData` and creates nothing; the same names under the core context are created
/// Clause: 5.5.7, 5.8.1.4, Table 5.5.2-1
#[gtest]
#[tokio::test]
async fn n_sub_cre_9028_a_name_the_context_maps_to_nothing_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let term_probe_type = namespace.entity_type("TermProbe")?;
    let subscription_type_id = namespace.subscription_id("type")?;
    let subscription_watched_id = namespace.subscription_id("watched")?;
    let subscription_attributes_id = namespace.subscription_id("attributes")?;
    let subscription_geoproperty_id = namespace.subscription_id("geoproperty")?;
    let subscription_control_id = namespace.subscription_id("control")?;
    let nothing_type = broker
        .with_context(Context::Multiple(vec![ContextEntry::Inline(
            format!(r#"{{"{term_probe_type}": null}}"#).parse::<InlineContext>()?,
        )]))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let nothing_terms = broker
        .with_context(Context::Multiple(vec![ContextEntry::Inline(
            r#"{"speed": null, "Vehicle": null, "place": null}"#.parse::<InlineContext>()?,
        )]))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let subscription_type = Subscription::builder()
        .id(subscription_type_id.clone())
        .entities([term_probe_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).build()?).build()?)
        .build()?;
    let answer = nothing_type.with_responses().subscriptions().create(&subscription_type).send().await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_type_id)
        .send()
        .await;
    expect_that!(&answer_2, answered(StatusCode::NOT_FOUND));
    let subscription_watched = Subscription::builder()
        .id(subscription_watched_id.clone())
        .entities([term_probe_type.as_str()])
        .watched_attributes(["speed"])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).build()?).build()?)
        .build()?;
    let answer_3 = nothing_terms.with_responses().subscriptions().create(&subscription_watched).send().await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_4 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_watched_id)
        .send()
        .await;
    expect_that!(&answer_4, answered(StatusCode::NOT_FOUND));
    let subscription_attributes = Subscription::builder()
        .id(subscription_attributes_id.clone())
        .entities([term_probe_type.as_str()])
        .notification(
            NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).build()?)
                .attributes(["speed"])
                .build()?,
        )
        .build()?;
    let answer_5 = nothing_terms.with_responses().subscriptions().create(&subscription_attributes).send().await;
    expect_that!(&answer_5, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_6 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_attributes_id)
        .send()
        .await;
    expect_that!(&answer_6, answered(StatusCode::NOT_FOUND));
    let subscription_geoproperty = Subscription::builder()
        .id(subscription_geoproperty_id.clone())
        .entities([term_probe_type.as_str()])
        .geo_q(
            geoquery::near_max(10, Shape::Point(Position::try_new(1.0, 2.0)?))
                .geoproperty("place")
                .build()?,
        )
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).build()?).build()?)
        .build()?;
    let answer_7 = nothing_terms.with_responses().subscriptions().create(&subscription_geoproperty).send().await;
    expect_that!(&answer_7, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_8 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_geoproperty_id)
        .send()
        .await;
    expect_that!(&answer_8, answered(StatusCode::NOT_FOUND));
    let subscription_control = Subscription::builder()
        .id(subscription_control_id.clone())
        .entities([term_probe_type.as_str()])
        .watched_attributes(["speed"])
        .notification(
            NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).build()?)
                .attributes(["speed"])
                .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription_control).send().await?;
    let stored = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_control_id)
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
    broker.subscriptions().delete(&subscription_control_id).send().await?;
    Ok(())
}
