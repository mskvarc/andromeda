//! Every write of an existing entity admits an attribute at the limit.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    nesting::{object_chain, object_chain_text},
    outcome::Done,
    problem::answered,
    wire::request_to,
};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::CONTENT_TYPE};
use ngsi_ld::{
    http::headers::Accept,
    model::{Attribute, AttributeName, Entity, EntityFragment, NgsiLdMediaType, Property},
};

/// Pins: Update Attributes, Append Attributes, Partial Attribute Update, Replace Attribute and Merge Entity each
/// with p nesting 32 levels answer 204 and leave p 32 levels deep
/// Clause: 5.6.2.4, 5.6.3.4, 5.6.4.4, 5.6.19.4, 5.6.17.4
#[gtest]
#[tokio::test]
async fn a_lim_dep_9010_every_write_of_an_existing_entity_admits_an_attribute_at_the_limit() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let nesting_probe_id = namespace.urn("NestingProbe", 1)?;
    let nesting_probe = Entity::builder()
        .id(nesting_probe_id.clone())
        .entity_type(namespace.entity_type("NestingProbe")?)
        .property("p", object_chain(1, "leaf"))
        .build()?;
    broker.entities().create(&nesting_probe).send().await?.done()?;
    let reset = async || -> Result<()> {
        let fragment = EntityFragment::builder().property("p", object_chain(1, "leaf")).build()?;
        broker.entities().update_attributes(&nesting_probe_id, &fragment).send().await?.done()?;
        Ok(())
    };
    let at_the_limit = async || -> Result<()> {
        let retrieved = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .entities()
            .retrieve(&nesting_probe_id)
            .send()
            .await?;
        verify_that!(retrieved.body(), has_attribute("p", any_attribute().value_member(["k"; 32], "leaf")))
    };
    let fragment = EntityFragment::builder().property("p", object_chain(32, "leaf")).build()?;
    broker.entities().update_attributes(&nesting_probe_id, &fragment).send().await?.done()?;
    at_the_limit().await?;
    reset().await?;
    broker.entities().append_attributes(&nesting_probe_id, &fragment).send().await?.done()?;
    at_the_limit().await?;
    reset().await?;
    // The model writes a value-only attribute fragment as the bare value, so the body is sent as written.
    let request = request_to(&broker, Method::PATCH, &format!("entities/{nesting_probe_id}/attrs/p"))?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .body(Bytes::from(format!(r#"{{"value": {}}}"#, object_chain_text(32, "leaf"))));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::NO_CONTENT));
    at_the_limit().await?;
    reset().await?;
    let attribute = Attribute::from(Property::builder(object_chain(32, "leaf")).build()?);
    broker
        .entities()
        .replace_attribute(&nesting_probe_id, &"p".parse::<AttributeName>()?, &attribute)
        .send()
        .await?
        .done()?;
    at_the_limit().await?;
    reset().await?;
    broker.entities().merge(&nesting_probe_id, &fragment).send().await?.done()?;
    at_the_limit().await?;
    reset().await?;
    Ok(())
}
