//! Expand values expands a quoted string through the request context.

use andromeda_test_suite::{broker, hosted_context::HostedContext, identity_matcher::has_id, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{AttributeName, Context, ContextEntry, Entity, InlineContext, NgsiLdMediaType},
    query::q,
};

/// Pins: `expandValues=status` expands the quoted string of q status=="active" through the request @context, where
/// status is coerced as @vocab, so it matches the IRI the stored value holds
/// Clause: 4.9, 5.7.2.4, 6.4.3.2, 5.5.7
/// Recorded: Table 6.4.3.2-1 says the values of the attributes `expandValues` names "should be expanded against the
/// supplied @context using JSON-LD type coercion prior to executing the query", which does not depend on how the
/// value is written
#[gtest]
#[tokio::test]
async fn n_qry_qlg_4013_expand_values_expands_a_quoted_string_through_the_request_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let device_type = namespace.entity_type("Device")?;
    let device_id = namespace.urn("Device", 1)?;
    let context = Context::Single(ContextEntry::Inline(
        format!(
            r#"{{"status": {{"@id": "https://test.andromeda/{namespace}/status", "@type": "@vocab"}}, "active": "https://test.andromeda/{namespace}/state/active"}}"#
        )
        .parse::<InlineContext>()?,
    ));
    let hosted = HostedContext::add(&broker, &context).await?;
    let linked = broker.with_context(Context::Single(hosted.entry()?))?;
    let device = Entity::builder()
        .id(device_id.clone())
        .entity_type(device_type.clone())
        .property("status", format!("https://test.andromeda/{namespace}/state/active"))
        .build()?;
    linked.entities().create(&device).send().await?.done()?;
    let page = linked
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&device_type)
        .q(q::attr("status").eq("active"))
        .expand_values(vec!["status".parse::<AttributeName>()?])
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), contains(has_id(&device_id)));
    Ok(())
}
