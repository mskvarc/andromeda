//! A Context Source Registration Subscription created with an unquoted prefixed name in its csf gives it back unquoted
//! under the prefix and as the quoted absolute IRI, which holds a dot, under no @context.

use andromeda_test_suite::{
    body_json::json_equals,
    broker,
    config::SuiteConfig,
    hosted_context::HostedContext,
    namespace::Namespace,
    problem::answered,
    received_subscription::retrieved_as_received,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, operation::CreateSubscription},
    model::{Context, ContextEntry, InlineContext, NgsiLdMediaType},
    subscription::{Endpoint, NotificationParams, RegistrationSubscription, Subscription},
};

/// Pins: a Context Source Registration Subscription created with csf=ex:tier=="gold" under an @context holding the
/// prefix ex is retrieved under that @context with ex:tier=="gold" and with no Link header with
/// `"https://example.org/<namespace>/tier"=="gold"`
/// Clause: 4.9, 4.6.2, 5.5.7, 5.11.2, 5.11.4
/// Recorded: Andromeda reads a csf by the path grammar of q, an unquoted name through `-`, `:`, `/` and `#` wherever
/// an unquoted colon has no meaning of its own, and writes a name back compacted, unquoted where it reads back
/// unquoted as itself and as a `quotedStr` otherwise
#[gtest]
#[tokio::test]
async fn a_css_qry_9101_an_unquoted_prefixed_name_in_a_created_csf_reads_back_unquoted_where_it_reads_back_so() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let prefixed = HostedContext::add(
        &broker,
        &Context::Single(ContextEntry::Inline(
            format!(r#"{{"ex":"https://example.org/{namespace}/"}}"#).parse::<InlineContext>()?,
        )),
    )
    .await?;
    let linked = broker.with_context(Context::Single(prefixed.entry()?))?;
    let subscription_id = namespace.subscription_id(1)?;
    let subscription = Subscription::registration_builder()
        .id(subscription_id.clone())
        .entities([namespace.entity_type("Sensor")?.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    // The q model writes a name only as an unquoted AttrName, so `csf` is written as text.
    let document = UncheckedDocument::try_from(subscription.render())?.set_text("csf", r#"ex:tier=="gold""#);
    let request = linked.unchecked(&CreateSubscription { subscription: &subscription })?.document(&document);
    expect_that!(&linked.send_unchecked(request).await, answered(StatusCode::CREATED));
    let quoted_iri = format!(r#""https://example.org/{namespace}/tier"=="gold""#);
    // The q model reads a name only as an unquoted AttrName, so a csf naming a prefixed name or an IRI is read as
    // received.
    for (reader, read) in [(&linked, r#"ex:tier=="gold""#), (&broker, quoted_iri.as_str())] {
        let retrieved = retrieved_as_received(reader, &subscription_id, RegistrationSubscription).await?;
        expect_that!(retrieved.body(), json_equals("$.csf", read), "{read}");
    }
    broker.registration_subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
