//! Query Subscriptions lists a Subscription's q naming a dot-free IRI that holds `;` as that one name.

use andromeda_test_suite::{
    body_json::{json_count, json_equals},
    broker,
    config::SuiteConfig,
    hosted_context::HostedContext,
    namespace::Namespace,
    received_subscription::listed_as_received,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Context, ContextEntry, InlineContext, NgsiLdMediaType},
    query::q,
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Query Subscriptions over a tenant holding one Subscription, created with q=speed>50 under an @context mapping
/// speed to `urn:<token>:a;b`, lists it under that @context with q speed>50 and with no Link header with
/// q "urn:<token>:a;b">50
/// Clause: 4.9, 5.5.7, 5.8.4
/// Recorded: clause 5.5.7 renders a name no term of the reader's @context matches as its Fully Qualified Name, and `;`
/// joins two Query Terms; Andromeda writes every name that is no `AttrName` as a `quotedStr`, so the IRI reads back as
/// one name
/// Tenant: private (Query Subscriptions lists every subscription of the tenant, so in the shared tenant the
/// subscriptions of other files fill the listing; clause 4.14 keeps each tenant's subscriptions its own)
#[gtest]
#[tokio::test]
async fn a_sub_qry_9088_query_subscriptions_lists_a_q_naming_a_dot_free_iri_with_a_semicolon_as_one_name() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let tenant = broker::private_tenant(&broker, &namespace).await?;
    let iri = format!("urn:{}:a;b", namespace.token());
    let termed = HostedContext::add(
        &tenant,
        &Context::Single(ContextEntry::Inline(format!(r#"{{"speed":"{iri}"}}"#).parse::<InlineContext>()?)),
    )
    .await?;
    let writer = tenant.with_context(Context::Single(termed.entry()?))?;
    let subscription_id = namespace.subscription_id(1)?;
    let speed_above_50 = q::attr("speed").gt(50).build()?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([namespace.entity_type("Car")?.as_str()])
        .q(speed_above_50.clone())
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    writer.subscriptions().create(&subscription).send().await?;
    let listed = writer
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .query()
        .send()
        .await?;
    expect_that!(listed.outcome.items(), len(eq(1)));
    expect_that!(listed.outcome.items().first().and_then(|listed| listed.q()), some(eq(&speed_above_50)));
    // The q model reads a name only as an unquoted AttrName, so the listing naming the IRI is read as received.
    let quoted_iri = format!(r#""{iri}">50"#);
    let received = listed_as_received(&tenant).await?;
    expect_that!(received.body(), json_count("$", 1));
    expect_that!(received.body(), json_equals("$[0].q", quoted_iri.as_str()));
    tenant.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
