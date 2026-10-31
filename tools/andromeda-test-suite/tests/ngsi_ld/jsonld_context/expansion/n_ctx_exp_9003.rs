//! An unretrievable context outranks a refused payload.

use andromeda_test_suite::{broker, config::SuiteConfig, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use iri_rs::IriRefBuf;
use ngsi_ld::{
    http::{UncheckedDocument, operation::CreateEntity},
    model::{Context, ContextEntry, Entity, ErrorType, Lossless, RawJson},
};

/// Pins: Create Entity under an unretrievable @context of a payload whose Attribute type would be refused answers
/// 504 `LdContextNotAvailable`, not `BadRequestData`
/// Clause: 5.5.4, 5.5.6, 6.3.2, 5.6.1.4
///
/// Invalid type: `NotAnAttributeType` (the refused attribute type the unretrievable context outranks)
#[gtest]
#[tokio::test]
async fn n_ctx_exp_9003_an_unretrievable_context_outranks_a_refused_payload() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let linked = broker.with_context(Context::Single(ContextEntry::Reference(IriRefBuf::new(format!(
        "{unreachable}/ctx/{namespace}/unreachable.jsonld"
    ))?)))?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder().id(building_id).entity_type(building_type).build()?;
    // The model refuses it (unknown attribute type `NotAnAttributeType` at name), so `name` is written as text.
    let name: RawJson = r#"{"type": "NotAnAttributeType", "value": "Reichstag"}"#.parse()?;
    let document = UncheckedDocument::from_entity(&building, Lossless::Normalized).set_member("name", &name);
    let request = linked.unchecked(&CreateEntity::builder().entity(&building).build())?.document(&document);
    let answer = linked.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::GATEWAY_TIMEOUT, ErrorType::LdContextNotAvailable));
    Ok(())
}
