//! A delete attribute under an unretrievable context link is ld context not available.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    config::SuiteConfig,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
    problem::problem,
};
use googletest::prelude::*;
use http::StatusCode;
use iri_rs::IriRefBuf;
use ngsi_ld::{
    http::headers::Accept,
    model::{AttributeName, Context, ContextEntry, Entity, ErrorType, NgsiLdMediaType},
};

/// Pins: Delete Attribute under a context Link that cannot be retrieved answers 504 `LdContextNotAvailable` and
/// leaves the Attribute in place
/// Clause: 6.3.5, 5.5.6, 5.6.5.4
#[gtest]
#[tokio::test]
async fn n_ctx_exp_9057_a_delete_attribute_under_an_unretrievable_context_link_is_ld_context_not_available() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let linked = broker.with_context(Context::Single(ContextEntry::Reference(IriRefBuf::new(format!(
        "{unreachable}/contexts/{namespace}/missing.jsonld"
    ))?)))?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("speed", 30)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let answer = linked
        .with_responses()
        .entities()
        .delete_attribute(&vehicle_id, &"speed".parse::<AttributeName>()?)
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::GATEWAY_TIMEOUT, ErrorType::LdContextNotAvailable));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("speed", any_attribute().value(30)));
    Ok(())
}
