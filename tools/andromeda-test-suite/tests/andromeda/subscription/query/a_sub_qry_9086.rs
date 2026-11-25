//! A Subscription's q hop with a type list reads back with every name quoted under no @context.

use andromeda_test_suite::{
    body_json::json_equals,
    broker,
    config::SuiteConfig,
    hosted_context::HostedContext,
    identity_matcher::ids_in_order,
    namespace::Namespace,
    outcome::Done,
    quoted_name_query::{context_link, query_by_text, retrieve_by_text},
    unchecked_page::entities,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    model::{Context, ContextEntry, Entity, EntityTypeName, InlineContext, NgsiLdMediaType},
    query::q,
    subscription::{Endpoint, NotificationParams, Subscription},
};
use std::slice;

/// Pins: a Subscription created with q=sensor{Device:humidity}==40 under an @context holding the terms sensor,
/// humidity and Device is retrieved under that @context with sensor{Device:humidity}==40 and with no Link header with
/// "<sensor IRI>"{"<Device IRI>":"<humidity IRI>"}==40, and each q returned, sent to Query Entities with join and
/// joinLevel under the @context it was returned for, selects the weather station the first form selects
/// Clause: 4.9, 5.5.7, 5.7.2.4, 5.8.3
#[gtest]
#[tokio::test]
async fn a_sub_qry_9086_a_subscription_q_hop_with_a_type_list_reads_back_quoted_under_no_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let vocabulary = format!("https://example.org/{namespace}");
    let termed = HostedContext::add(
        &broker,
        &Context::Single(ContextEntry::Inline(
            format!(r#"{{"sensor":"{vocabulary}/sensor","humidity":"{vocabulary}/humidity","Device":"{vocabulary}/Device"}}"#).parse::<InlineContext>()?,
        )),
    )
    .await?;
    let writer = broker.with_context(Context::Single(termed.entry()?))?;
    let station_type = namespace.entity_type("WeatherStation")?;
    let station_id = namespace.urn("WeatherStation", 123)?;
    let device_id = namespace.urn("Device", 345)?;
    let device = Entity::builder()
        .id(device_id.clone())
        .entity_type("Device".parse::<EntityTypeName>()?)
        .property("humidity", 40)
        .build()?;
    writer.entities().create(&device).send().await?.done()?;
    let station = Entity::builder()
        .id(station_id.clone())
        .entity_type(station_type.clone())
        .relationship("sensor", device_id)
        .build()?;
    writer.entities().create(&station).send().await?.done()?;
    let subscription_id = namespace.subscription_id(1)?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([station_type.as_str()])
        .q(q::attr("sensor").linked_typed(["Device"], "humidity").eq(40).build()?)
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    writer.subscriptions().create(&subscription).send().await?;
    let expanded = format!(r#""{vocabulary}/sensor"{{"{vocabulary}/Device":"{vocabulary}/humidity"}}==40"#);
    let readers = [(Some(context_link(&termed)?), "sensor{Device:humidity}==40"), (None, expanded.as_str())];
    let joined = [("join", "inline"), ("joinLevel", "1")];
    // The q model reads a name only as an unquoted AttrName, so the Subscription and the queries are sent as written.
    for (link, read) in &readers {
        let retrieved = retrieve_by_text(&broker, &subscription_id, link.as_ref()).await?;
        expect_that!(retrieved.status(), eq(StatusCode::OK), "{read}");
        expect_that!(retrieved.body(), json_equals("$.q", *read), "{read}");
        let selected = query_by_text(&broker, &station_type, read, &joined, link.as_ref()).await?;
        expect_that!(selected.status(), eq(StatusCode::OK), "{read}");
        expect_that!(entities(&selected)?.as_slice(), ids_in_order(slice::from_ref(&station_id)), "{read}");
    }
    broker.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
