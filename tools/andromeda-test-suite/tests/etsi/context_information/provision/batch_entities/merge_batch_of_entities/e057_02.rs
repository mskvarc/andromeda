//! A batch of fragments merged into two existing entities and one absent entity answers 207, names the absent one in
//! error and merges the others (clause 5.6.17).

use crate::{
    batch_operations::{expect_partial_batch, partial_batch_outcome},
    building_documents::weather_building_json,
    building_payloads::{BUILDING_AIR_QUALITY_WEATHER_TERMS, building_simple_attributes, building_with_weather_sensor},
    entity_operations::{create_entity_with_context_string, delete_entities, ld_json_body_client, query_entities_json},
};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace, vocabulary::Vocabulary};
use googletest::prelude::*;
use ngsi_ld::model::NgsiLdMediaType;
use serde_json::json;

/// Pins: Check that you can merge a batch of non-existing and existing entities
/// Clause: 5.6.17
#[gtest]
#[tokio::test]
async fn e057_02_01_merge_a_batch_of_existing_and_non_existing_entities_returns_207() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_AIR_QUALITY_WEATHER_TERMS).await?;
    create_entity_with_context_string(&broker, &vocabulary, &building_simple_attributes(&namespace, 1)?).await?;
    create_entity_with_context_string(&broker, &vocabulary, &building_simple_attributes(&namespace, 2)?).await?;
    let fragments = [
        building_with_weather_sensor(&namespace, 1)?.into_fragment(),
        building_with_weather_sensor(&namespace, 2)?.into_fragment(),
        building_with_weather_sensor(&namespace, 3)?.into_fragment(),
    ];
    let answer = ld_json_body_client(&broker, &vocabulary)?
        .with_responses()
        .entities()
        .batch_merge(&fragments)
        .send()
        .await;
    let result = partial_batch_outcome(answer)?;
    expect_partial_batch(
        &result,
        &[&namespace.urn("Building", 1)?, &namespace.urn("Building", 2)?],
        &namespace.urn("Building", 3)?,
    )?;
    let client = broker.with_context(vocabulary.context())?.with_accept(NgsiLdMediaType::JsonLd);
    let body = query_entities_json(&client, vec![namespace.urn("Building", 1)?, namespace.urn("Building", 2)?], "Building").await?;
    let context = vocabulary.url().as_str();
    expect_that!(
        &body,
        matches_document(json!([
            weather_building_json(&namespace, 1, context),
            weather_building_json(&namespace, 2, context)
        ]))
    );
    delete_entities(&broker, &[&namespace.urn("Building", 1)?, &namespace.urn("Building", 2)?]).await;
    Ok(())
}
