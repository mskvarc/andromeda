//! A Subscription's q naming a dot-free IRI that holds `;` reads back as that one name.

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
    model::{Context, ContextEntry, Entity, InlineContext, NgsiLdMediaType},
    query::q,
    subscription::{Endpoint, NotificationParams, Subscription},
};
use std::slice;

/// Pins: a Subscription created with q=speed>50 under an @context mapping speed to `urn:<token>:a;b` is retrieved
/// under that @context with speed>50 and with no Link header with "urn:<token>:a;b">50, and each q returned, sent to
/// Query Entities under the @context it was returned for, selects the Car speed>50 selects
/// Clause: 4.9, 5.5.7, 5.8.3
/// Recorded: clause 5.5.7 renders a name no term of the reader's @context matches as its Fully Qualified Name, and `;`
/// joins two Query Terms; Andromeda writes every name that is no `AttrName` as a `quotedStr`, so the IRI reads back as
/// one name
#[gtest]
#[tokio::test]
async fn a_sub_qry_9087_a_subscription_q_naming_a_dot_free_iri_with_a_semicolon_reads_back_as_one_name() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let iri = format!("urn:{}:a;b", namespace.token());
    let termed = HostedContext::add(
        &broker,
        &Context::Single(ContextEntry::Inline(format!(r#"{{"speed":"{iri}"}}"#).parse::<InlineContext>()?)),
    )
    .await?;
    let writer = broker.with_context(Context::Single(termed.entry()?))?;
    let car_type = namespace.entity_type("Car")?;
    let fast_id = namespace.urn("Car", "fast")?;
    for (id, speed) in [(fast_id.clone(), 80), (namespace.urn("Car", "slow")?, 20)] {
        let car = Entity::builder().id(id).entity_type(car_type.clone()).property("speed", speed).build()?;
        writer.entities().create(&car).send().await?.done()?;
    }
    let subscription_id = namespace.subscription_id(1)?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([car_type.as_str()])
        .q(q::attr("speed").gt(50).build()?)
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    writer.subscriptions().create(&subscription).send().await?;
    let quoted_iri = format!(r#""{iri}">50"#);
    let readers = [(Some(context_link(&termed)?), "speed>50"), (None, quoted_iri.as_str())];
    // The q model reads a name only as an unquoted AttrName, so the queries are sent as written.
    for (link, read) in &readers {
        let retrieved = retrieve_by_text(&broker, &subscription_id, link.as_ref()).await?;
        expect_that!(retrieved.status(), eq(StatusCode::OK), "{read}");
        expect_that!(retrieved.body(), json_equals("$.q", *read), "{read}");
        let selected = query_by_text(&broker, &car_type, read, &[], link.as_ref()).await?;
        expect_that!(selected.status(), eq(StatusCode::OK), "{read}");
        expect_that!(entities(&selected)?.as_slice(), ids_in_order(slice::from_ref(&fast_id)), "{read}");
    }
    broker.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
