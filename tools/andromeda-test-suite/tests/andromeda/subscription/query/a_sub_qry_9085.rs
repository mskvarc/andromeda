//! A Subscription's q reads back in the terms of the reader's @context, and a name it has no term for comes back quoted.

use andromeda_test_suite::{
    body_json::json_equals,
    broker,
    config::SuiteConfig,
    hosted_context::HostedContext,
    identity_matcher::ids_in_order,
    namespace::Namespace,
    outcome::Done,
    received_subscription::retrieved_as_received,
    written_parameters::send_written,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{QueryParameter, headers::Accept, operation::QueryEntities},
    model::{Context, ContextEntry, Entity, InlineContext, Lossless, NgsiLdMediaType},
    query::{builders::SelectorTypeBuilder, q, q::Query},
    subscription::{Endpoint, EntitySubscription, NotificationParams, Subscription},
};
use std::slice;

/// Pins: a Subscription created with q=speed>50 under an @context holding the term speed is retrieved under that
/// @context with speed>50, with no Link header with "<the IRI>">50, under an @context holding only a prefix for the
/// namespace with "ex:speed">50 and under an @context holding the term velocity for the IRI with velocity>50, and each
/// q returned, sent to Query Entities under the @context it was returned for, selects the Car speed>50 selects
/// Clause: 4.9, 5.5.7, 5.8.3
/// Recorded: clause 5.5.7 renders a name no term of the reader's @context matches as its Fully Qualified Name; Andromeda
/// writes a name back compacted, unquoted when it is an `AttrName` and as a `quotedStr` otherwise
#[gtest]
#[tokio::test]
async fn a_sub_qry_9085_a_subscription_q_reads_back_in_the_reader_terms_and_quoted_where_it_has_none() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let iri = format!("https://example.org/{namespace}/speed");
    let context = |members: String| -> Result<Context> { Ok(Context::Single(ContextEntry::Inline(members.parse::<InlineContext>()?))) };
    let termed = HostedContext::add(&broker, &context(format!(r#"{{"speed":"{iri}"}}"#))?).await?;
    let prefixed = HostedContext::add(&broker, &context(format!(r#"{{"ex":"https://example.org/{namespace}/"}}"#))?).await?;
    let renamed = HostedContext::add(&broker, &context(format!(r#"{{"velocity":"{iri}"}}"#))?).await?;
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
    let selection = SelectorTypeBuilder::from(&car_type).build()?;
    let renamed_reader = broker.with_context(Context::Single(renamed.entry()?))?;
    for (reader, read) in [(&writer, "speed>50"), (&renamed_reader, "velocity>50")] {
        let expected: Query = read.parse()?;
        let retrieved = reader
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .subscriptions()
            .retrieve(&subscription_id)
            .send()
            .await?;
        expect_that!(retrieved.q(), some(eq(&expected)), "{read}");
        let query = QueryEntities::builder()
            .entity_type(&selection)
            .q(&expected)
            .format(Lossless::Normalized)
            .build();
        let page = send_written(reader, &query, &[]).await?;
        expect_that!(page.items(), ids_in_order(slice::from_ref(&fast_id)), "{read}");
    }
    let prefixed_reader = broker.with_context(Context::Single(prefixed.entry()?))?;
    let quoted_iri = format!(r#""{iri}">50"#);
    let query = QueryEntities::builder().entity_type(&selection).format(Lossless::Normalized).build();
    // The q model reads a name only as an unquoted AttrName, so a q naming an IRI or a prefixed name is read and sent as
    // written.
    for (reader, read) in [(&broker, quoted_iri.as_str()), (&prefixed_reader, r#""ex:speed">50"#)] {
        let retrieved = retrieved_as_received(reader, &subscription_id, EntitySubscription).await?;
        expect_that!(retrieved.body(), json_equals("$.q", read), "{read}");
        let page = send_written(reader, &query, &[(QueryParameter::Q, read)]).await?;
        expect_that!(page.items(), ids_in_order(slice::from_ref(&fast_id)), "{read}");
    }
    broker.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
