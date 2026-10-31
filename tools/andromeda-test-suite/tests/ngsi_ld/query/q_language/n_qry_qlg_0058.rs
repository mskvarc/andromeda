//! A full language tag selector is bad request data.

use andromeda_test_suite::{
    broker,
    identity_matcher::has_id,
    namespace::Namespace,
    notification_sink::NotificationSink,
    nth::nth,
    outcome::Done,
    problem::problem,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::QueryMethod},
    model::{Entity, ErrorType, LanguageMap, LanguageProperty, NgsiLdMediaType},
    query::{ordering::OrderBy, q, temporal_query},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Query Entities by GET and by POST, Query Temporal Evolution of Entities and Create Subscription with a q
/// whose bracket names a full language tag such as label`[en-GB]` or label`[zh-Hant-TW]`, and Query Entities with
/// orderBy=label`[en-GB]`, answer `BadRequestData`, while label`[en]` is still read as a target language
/// Clause: 4.9, 4.5.18, 4.23, 5.7.2.4, 5.8.1
/// Recorded: the clause 4.9 ABNF writes the bracket as `[DottedPath]`, whose `AttrName` admits no hyphen; Annex
/// C.5.13.2 prints marque`[en-GB]`, but Annex C is informative and the ABNF is normative
#[gtest]
#[tokio::test]
async fn n_qry_qlg_0058_a_full_language_tag_selector_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let sensor_id = namespace.urn("Sensor", 1)?;
    let sensor_type = namespace.entity_type("Sensor")?;
    let subscription_id = namespace.subscription_id(1)?;
    let sensor = Entity::builder()
        .id(sensor_id.clone())
        .entity_type(sensor_type.clone())
        .language_property(
            "label",
            LanguageProperty::builder(LanguageMap::try_from_pairs([("en-GB", "Lift"), ("en", "Elevator")])?),
        )
        .build()?;
    broker.entities().create(&sensor).send().await?.done()?;
    let answer = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&sensor_type)
        .q(q::attr("label").member("en-GB").eq("Lift"))
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&sensor_type)
        .q(q::attr("label").member("zh-Hant-TW").eq("Lift"))
        .send()
        .await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    let answer_3 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&sensor_type)
        .ordering("label[en-GB]".parse::<OrderBy>()?)
        .send()
        .await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&sensor_type)
        .q(q::attr("label").member("en").eq("Elevator"))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&sensor_id)));
    let answer_4 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .method(QueryMethod::Post)
        .entity_type(&sensor_type)
        .q(q::attr("label").member("en-GB").eq("Lift"))
        .send()
        .await;
    expect_that!(&answer_4, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    let answer_5 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2000-01-01T00:00:00Z".parse()?))
        .entity_type(&sensor_type)
        .q(q::attr("label").member("en-GB").eq("Lift"))
        .send()
        .await;
    expect_that!(&answer_5, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([sensor_type.as_str()])
        .q(q::attr("label").member("en-GB").eq("Lift").build()?)
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    let answer_6 = broker.with_responses().subscriptions().create(&subscription).send().await;
    expect_that!(&answer_6, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    Ok(())
}
