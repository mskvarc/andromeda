//! A Subscription's q naming a dot-free IRI that holds `;` reads back as that one name.

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
    query::{builders::SelectorTypeBuilder, q},
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
    let speed_above_50 = q::attr("speed").gt(50).build()?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([car_type.as_str()])
        .q(speed_above_50.clone())
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    writer.subscriptions().create(&subscription).send().await?;
    let retrieved = writer
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await?;
    expect_that!(retrieved.q(), some(eq(&speed_above_50)));
    let selection = SelectorTypeBuilder::from(&car_type).build()?;
    let typed_query = QueryEntities::builder()
        .entity_type(&selection)
        .q(&speed_above_50)
        .format(Lossless::Normalized)
        .build();
    let typed_page = send_written(&writer, &typed_query, &[]).await?;
    expect_that!(typed_page.items(), ids_in_order(slice::from_ref(&fast_id)));
    let quoted_iri = format!(r#""{iri}">50"#);
    // The q model reads a name only as an unquoted AttrName, so the q naming the IRI is read and sent as written.
    let received = retrieved_as_received(&broker, &subscription_id).await?;
    expect_that!(received.body(), json_equals("$.q", quoted_iri.as_str()));
    let query = QueryEntities::builder().entity_type(&selection).format(Lossless::Normalized).build();
    let page = send_written(&broker, &query, &[(QueryParameter::Q, quoted_iri.as_str())]).await?;
    expect_that!(page.items(), ids_in_order(slice::from_ref(&fast_id)));
    broker.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
