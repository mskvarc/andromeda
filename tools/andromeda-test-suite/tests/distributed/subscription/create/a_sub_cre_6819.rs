//! A copy writes a q name its @context has no term for as a quoted absolute IRI.

use andromeda_test_suite::{
    body_json::json_equals,
    broker,
    config::SuiteConfig,
    context_source::ContextSource,
    hosted_context::HostedContext,
    namespace::Namespace,
    notification_sink::NotificationSink,
    relayed_copy::{placed_copy, register_copy_source},
};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Context, ContextEntry, InlineContext, NgsiLdMediaType},
    query::q,
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Create Subscription with q brand=="x" under an @context mapping brand to
/// `https://example.org/<namespace>/brand`, whose `jsonldContext` is the core @context, places a copy whose q is
/// `"https://example.org/<namespace>/brand"=="x"`
/// Clause: 4.9, 5.5.7, 5.8.1.4
/// Recorded: the copy is written under the Subscription's `jsonldContext`, which has no term for the IRI, so clause
/// 5.5.7 renders it as its Fully Qualified Name; Andromeda writes a name holding a dot as a `quotedStr`
/// Tenant: private (the Subscription's q lists back under the core @context as a quoted IRI, which the suite's client
/// cannot read, so in the shared tenant every other file's Query Subscriptions would fail to read its listing)
#[gtest]
#[tokio::test]
async fn a_sub_cre_6819_a_copy_writes_a_q_name_its_context_has_no_term_for_as_a_quoted_iri() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let sink = NotificationSink::start().await?;
    let source = ContextSource::start().await?;
    let tenant = broker::private_tenant(&broker, &namespace).await?;
    let iri = format!("https://example.org/{namespace}/brand");
    let termed = HostedContext::add(
        &tenant,
        &Context::Single(ContextEntry::Inline(format!(r#"{{"brand":"{iri}"}}"#).parse::<InlineContext>()?)),
    )
    .await?;
    let writer = tenant.with_context(Context::Single(termed.entry()?))?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    register_copy_source(&tenant, &source, &namespace, "s1").await?;
    let subscription = Subscription::builder()
        .id(namespace.subscription_id(1)?)
        .entities([vehicle_type.as_str()])
        .q(q::attr("brand").eq("x").build()?)
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .jsonld_context(config.core_context().clone())
        .build()?;
    writer.subscriptions().create(&subscription).send().await?;
    let copy = placed_copy(&source, "s1", &vehicle_type).await?;
    // The q model reads a name only as an unquoted AttrName, so the q of the copy is read as received.
    let quoted_iri = format!(r#""{iri}"=="x""#);
    expect_that!(copy.body.as_slice(), json_equals("$.q", quoted_iri.as_str()));
    Ok(())
}
