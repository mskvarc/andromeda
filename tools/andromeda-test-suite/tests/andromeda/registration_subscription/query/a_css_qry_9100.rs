//! A Context Source Registration Subscription's csf reads back in the terms of the reader's @context, and a name it has
//! no term for comes back as a compact IRI or an absolute IRI, quoted where it holds a dot.

use andromeda_test_suite::{
    body_json::json_equals,
    broker,
    config::SuiteConfig,
    hosted_context::HostedContext,
    namespace::Namespace,
    received_subscription::retrieved_as_received,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Context, ContextEntry, InlineContext, NgsiLdMediaType},
    query::q,
    subscription::{Endpoint, NotificationParams, RegistrationSubscription, Subscription},
};

/// Pins: a Context Source Registration Subscription created with csf=tier=="gold" under an @context holding the term
/// tier is retrieved under that @context with tier=="gold", with no Link header with "<the IRI>"=="gold", the IRI
/// holding a dot, and under an @context holding only a prefix for the namespace with ex:tier=="gold"
/// Clause: 4.9, 5.5.7, 5.11.2, 5.11.4
/// Recorded: clause 5.5.7 renders a name no term of the reader's @context matches as its Fully Qualified Name;
/// Andromeda writes a name of a csf back compacted, unquoted where it reads back unquoted as itself and as a
/// `quotedStr` otherwise
#[gtest]
#[tokio::test]
async fn a_css_qry_9100_a_csf_reads_back_in_the_reader_terms_and_unquoted_where_it_reads_back_so() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let iri = format!("https://example.org/{namespace}/tier");
    let context = |members: String| -> Result<Context> { Ok(Context::Single(ContextEntry::Inline(members.parse::<InlineContext>()?))) };
    let termed = HostedContext::add(&broker, &context(format!(r#"{{"tier":"{iri}"}}"#))?).await?;
    let prefixed = HostedContext::add(&broker, &context(format!(r#"{{"ex":"https://example.org/{namespace}/"}}"#))?).await?;
    let writer = broker.with_context(Context::Single(termed.entry()?))?;
    let subscription_id = namespace.subscription_id(1)?;
    let subscription = Subscription::registration_builder()
        .id(subscription_id.clone())
        .entities([namespace.entity_type("Sensor")?.as_str()])
        .csf(q::attr("tier").eq("gold").build()?)
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    writer.registration_subscriptions().create(&subscription).send().await?;
    let stored = writer
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registration_subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await?;
    expect_that!(stored.csf().map(ToString::to_string), some(eq(r#"tier=="gold""#)));
    let prefixed_reader = broker.with_context(Context::Single(prefixed.entry()?))?;
    let quoted_iri = format!(r#""{iri}"=="gold""#);
    // The q model reads a name only as an unquoted AttrName, so a csf naming an IRI or a prefixed name is read as
    // received.
    for (reader, read) in [(&broker, quoted_iri.as_str()), (&prefixed_reader, r#"ex:tier=="gold""#)] {
        let retrieved = retrieved_as_received(reader, &subscription_id, RegistrationSubscription).await?;
        expect_that!(retrieved.body(), json_equals("$.csf", read), "{read}");
    }
    broker.registration_subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
