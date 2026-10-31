//! Entities queried by id are returned in a simplified representation (clause 6.3.7).

use crate::{
    building_payloads::{BUILDING_SIMPLE_TERMS, building_simple_attributes},
    entity_operations::{create_entity, delete_entities},
};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace, problem::answered, vocabulary::Vocabulary, wire::json_body};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::http::headers::Accept;
use serde_json::json;

/// Pins: Check that the queried entities by Id can be returned in a simplified representation
/// Clause: 6.3.7
#[gtest]
#[tokio::test]
async fn e019_04_01_query_entities_in_a_simplified_representation() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_SIMPLE_TERMS).await?;
    create_entity(&broker, &vocabulary, &building_simple_attributes(&namespace, 1)?).await?;
    create_entity(&broker, &vocabulary, &building_simple_attributes(&namespace, 2)?).await?;
    create_entity(&broker, &vocabulary, &building_simple_attributes(&namespace, 3)?).await?;
    let received = broker
        .with_accept(Accept::any())
        .with_responses()
        .entities()
        .query_simplified()
        .ids(vec![namespace.urn("Building", 1)?, namespace.urn("Building", 2)?])
        .entity_type(namespace.expanded_term("Building")?.as_str())
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    let building = |n: u32| -> Result<serde_json::Value> {
        Ok(json!({
            "id": namespace.urn_text("Building", n),
            "type": namespace.expanded_term("Building")?,
            namespace.expanded_term("name")?: "Eiffel Tower",
            namespace.expanded_term("airQualityLevel")?: 4,
            namespace.expanded_term("almostFull")?: false,
            namespace.expanded_term("subCategory")?: "tourism",
        }))
    };
    expect_that!(&body, matches_document(json!([building(1)?, building(2)?])).hiding("instanceId"));
    delete_entities(
        &broker,
        &[&namespace.urn("Building", 1)?, &namespace.urn("Building", 2)?, &namespace.urn("Building", 3)?],
    )
    .await;
    Ok(())
}
