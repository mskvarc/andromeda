//! A purge under an unretrievable context link is ld context not available.

use andromeda_test_suite::{broker, config::SuiteConfig, namespace::Namespace, outcome::Done, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use iri_rs::IriRefBuf;
use ngsi_ld::{
    http::headers::Accept,
    model::{Context, ContextEntry, Entity, EntityTypeName, ErrorType, NgsiLdMediaType},
    query::Locality,
};

/// Pins: a local Purge Entities selecting by a type written as a full IRI, which expands no term, under a context
/// Link that cannot be retrieved answers 504 `LdContextNotAvailable` and purges no Entity
/// Clause: 6.3.5, 5.5.6, 5.6.21.4
#[gtest]
#[tokio::test]
async fn n_ctx_exp_9058_a_purge_under_an_unretrievable_context_link_is_ld_context_not_available() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let building_type = format!("https://test.andromeda/{namespace}/Building").parse::<EntityTypeName>()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_2_id = namespace.urn("Building", 2)?;
    let linked = broker.with_context(Context::Single(ContextEntry::Reference(IriRefBuf::new(format!(
        "{unreachable}/contexts/{namespace}/missing.jsonld"
    ))?)))?;
    let entities = [
        Entity::builder().id(building_id.clone()).entity_type(building_type.clone()).build()?,
        Entity::builder().id(building_2_id.clone()).entity_type(building_type.clone()).build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let answer = linked
        .with_responses()
        .entities()
        .purge()
        .entity_type(&building_type)
        .locality(Locality::Local)
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::GATEWAY_TIMEOUT, ErrorType::LdContextNotAvailable));
    for id in [&building_id, &building_2_id] {
        let retrieved = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).entities().retrieve(id).send().await?;
        let entity = retrieved.body();
        expect_that!(entity.id, eq(id));
    }
    Ok(())
}
