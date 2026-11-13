//! A delete entity under an unretrievable context link is ld context not available.

use andromeda_test_suite::{broker, config::SuiteConfig, namespace::Namespace, outcome::Done, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use iri_rs::IriRefBuf;
use ngsi_ld::{
    http::headers::Accept,
    model::{Context, ContextEntry, Entity, ErrorType, NgsiLdMediaType},
};

/// Pins: Delete Entity by id alone, which expands no term, under a context Link that cannot be retrieved answers 504
/// `LdContextNotAvailable` and leaves the Entity in place
/// Clause: 6.3.5, 5.5.6, 5.6.6.4
#[gtest]
#[tokio::test]
async fn n_ctx_exp_9056_a_delete_entity_under_an_unretrievable_context_link_is_ld_context_not_available() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let linked = broker.with_context(Context::Single(ContextEntry::Reference(IriRefBuf::new(format!(
        "{unreachable}/contexts/{namespace}/missing.jsonld"
    ))?)))?;
    let building = Entity::builder().id(building_id.clone()).entity_type(building_type).build()?;
    broker.entities().create(&building).send().await?.done()?;
    let answer = linked.with_responses().entities().delete(&building_id).send().await;
    expect_that!(&answer, problem(StatusCode::GATEWAY_TIMEOUT, ErrorType::LdContextNotAvailable));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity.id, eq(&building_id));
    Ok(())
}
