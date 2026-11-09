//! An entity is retrieved in the simplified representation (clause 6.3.7).

use crate::{building_payloads::building_simple_attributes, entity_operations::create_entity};
use andromeda_test_suite::{
    broker,
    comparison::{ContextRule, matches_document},
    namespace::Namespace,
    problem::answered,
    teardown::AnyAnswer,
    vocabulary::Vocabulary,
    wire::json_body,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::model::NgsiLdMediaType;
use serde_json::json;

/// The terms of the case's `@context`.
const TERMS: [&str; 5] = ["Building", "name", "subCategory", "airQualityLevel", "almostFull"];

/// Pins: Check that the queried entity by Id can be returned in a simplified representation
/// Clause: 6.3.7
#[gtest]
#[tokio::test]
async fn e018_04_01_get_an_entity_in_a_simplified_representation() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    let building = building_simple_attributes(&namespace, 1)?;
    create_entity(&broker, &vocabulary, &building).await?;
    let received = broker
        .with_accept(NgsiLdMediaType::JsonLd)
        .with_responses()
        .entities()
        .retrieve_simplified(&building.id)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    let expected = json!({
        "id": building.id.to_string(),
        "type": namespace.expanded_term("Building")?,
        namespace.expanded_term("name")?: "Eiffel Tower",
        namespace.expanded_term("subCategory")?: "tourism",
        namespace.expanded_term("airQualityLevel")?: 4,
        namespace.expanded_term("almostFull")?: false,
        "@context": "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld",
    });
    expect_that!(&body, matches_document(expected).context_rule(ContextRule::AnyCoreContextVersion));
    broker.entities().delete(&building.id).send().await.any_answer();
    broker.temporal().delete(&building.id).send().await.any_answer();
    Ok(())
}
