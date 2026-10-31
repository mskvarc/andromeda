//! A single create under an unretrievable context is ld context not available.

use andromeda_test_suite::{broker, config::SuiteConfig, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use iri_rs::IriRefBuf;
use ngsi_ld::{
    http::headers::Accept,
    model::{Context, ContextEntry, Entity, ErrorType, NgsiLdMediaType},
};

/// Pins: Create Entity whose only @context is a Link naming an unretrievable document answers 504
/// `LdContextNotAvailable` with a title, and stores no Entity
/// Clause: 5.5.6, 5.6.1.4, 6.3.2
#[gtest]
#[tokio::test]
async fn n_ent_cre_9034_a_single_create_under_an_unretrievable_context_is_ld_context_not_available() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let linked = broker.with_context(Context::Single(ContextEntry::Reference(IriRefBuf::new(format!(
        "{unreachable}/ctx/{namespace}/unreachable.jsonld"
    ))?)))?;
    let building_single_id = namespace.urn("Building", "single")?;
    let building_type = namespace.entity_type("Building")?;
    let building_single = Entity::builder()
        .id(building_single_id.clone())
        .entity_type(building_type)
        .property("name", "Reichstag")
        .build()?;
    let answer = linked.with_responses().entities().create(&building_single).send().await;
    expect_that!(&answer, problem(StatusCode::GATEWAY_TIMEOUT, ErrorType::LdContextNotAvailable).with_title());
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_single_id)
        .send()
        .await;
    expect_that!(&answer_2, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    Ok(())
}
