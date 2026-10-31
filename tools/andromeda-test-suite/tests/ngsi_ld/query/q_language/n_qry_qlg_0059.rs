//! A wildcard language range is bad request data.

use andromeda_test_suite::{
    broker,
    identity_matcher::has_id,
    namespace::Namespace,
    notification_sink::NotificationSink,
    nth::nth,
    outcome::Done,
    problem::problem,
    wire::request_to,
};
use bytes::Bytes;
use googletest::prelude::*;
use http::{
    HeaderValue,
    Method,
    StatusCode,
    header::{ACCEPT, CONTENT_TYPE},
};
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::CreateSubscription},
    model::{Entity, ErrorType, LanguageMap, LanguageProperty, NgsiLdMediaType, RawJson},
    query::q,
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Query Entities by GET and by POST, Query Temporal Evolution of Entities and Create Subscription with a q
/// whose language selector holds a wildcard range such as `[de-*-DE]` answer `BadRequestData` on a broker at the
/// clause 4.9 grammar, while the every-language marker `[*]` is still accepted
/// Clause: 4.9, 4.5.18, 5.7.2.4, 5.8.1
/// Recorded: the clause 4.9 grammar admits no * in a language selector; Andromeda reads a wildcard range only at
/// the language-ranges level of its `q_target_languages` extension, which this broker does not run
#[gtest]
#[tokio::test]
async fn n_qry_qlg_0059_a_wildcard_language_range_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let sensor_id = namespace.urn("Sensor", 1)?;
    let sensor_type = namespace.entity_type("Sensor")?;
    let subscription_id = namespace.subscription_id(1)?;
    let sensor = Entity::builder()
        .id(sensor_id.clone())
        .entity_type(sensor_type.clone())
        .language_property("label", LanguageProperty::builder(LanguageMap::try_from_pairs([("de-DE", "Lift")])?))
        .build()?;
    broker.entities().create(&sensor).send().await?.done()?;
    // The model refuses this q, so the query is sent as written.
    let request = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entities?type=Sensor_{namespace}&q=label%5Bde-%2A-DE%5D%3D%3D%22Lift%22"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&sensor_type)
        .q(q::attr("label").member("*").eq("Lift"))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&sensor_id)));
    // The model refuses a member of this document, so it is sent as written.
    let request_2 = request_to(&broker.with_accept(Accept::new(NgsiLdMediaType::Json)), Method::POST, "entityOperations/query")?.append_header(ACCEPT, HeaderValue::from_static("application/json")).append_header(CONTENT_TYPE, HeaderValue::from_static("application/json")).body(Bytes::from(format!("{{\n    \"type\": \"Query\",\n    \"entities\": [\n        {{\n            \"type\": \"Sensor_{namespace}\"\n        }}\n    ],\n    \"q\": \"label[de-*-DE]==\\\"Lift\\\"\"\n}}")));
    let answer_2 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // The model refuses this q, so the query is sent as written.
    let request_3 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("temporal/entities?type=Sensor_{namespace}&q=label%5Bde-%2A-DE%5D%3D%3D%22Lift%22&timerel=after&timeAt=2000-01-01T00%3A00%3A00Z"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_3 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_3).await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([sensor_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    // The model refuses it (invalid query language text at q), so `q` is written as text.
    let q: RawJson = r#""label[de-*-DE]==\"Lift\"""#.parse()?;
    let document = UncheckedDocument::try_from(subscription.render())?.set_member("q", &q);
    let request_4 = broker.unchecked(&CreateSubscription { subscription: &subscription })?.document(&document);
    let answer_4 = broker.send_unchecked(request_4).await;
    expect_that!(&answer_4, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    Ok(())
}
