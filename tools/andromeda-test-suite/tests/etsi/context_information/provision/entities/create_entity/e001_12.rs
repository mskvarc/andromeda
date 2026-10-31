//! An entity with an `observationSpace` `GeoProperty` is created and retrieved with it (clauses 5.6.1 and 4.7).

use crate::{
    entity_operations::{create_entity_answer, delete_entities, expect_created, retrieve_entity_json},
    geometry_payloads::point,
};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace, vocabulary::Vocabulary};
use googletest::prelude::*;
use ngsi_ld::model::{Entity, Input, NgsiLdMediaType};
use serde_json::json;

/// The Building `urn:ngsi-ld:Building:<ns>-1` whose `observationSpace` is the point 13.3986, 52.5547.
fn building_observation_space(namespace: &Namespace) -> Result<Entity<Input>> {
    Ok(Entity::builder()
        .id(namespace.urn("Building", 1)?)
        .entity_type("Building")
        .geo_property("observationSpace", point(13.3986, 52.5547)?)
        .build()?)
}

/// Pins: Check that one can create an entity with observationSpace geospatial Property
/// Clause: 5.6.1, 4.7
#[gtest]
#[tokio::test]
async fn e001_12_observation_space_geoproperty() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building"]).await?;
    let entity = building_observation_space(&namespace)?;
    let created = create_entity_answer(&broker, &vocabulary, &entity).await?;
    expect_created(&created, &entity.id);
    let client = broker.with_context(vocabulary.context())?.with_accept(NgsiLdMediaType::JsonLd);
    let body = retrieve_entity_json(&client, &entity.id).await?;
    expect_that!(
        &body,
        matches_document(json!({
            "id": entity.id.to_string(),
            "type": "Building",
            "observationSpace": {
                "type": "GeoProperty",
                "value": {"type": "Point", "coordinates": [13.3986, 52.5547]}
            },
            "@context": vocabulary.url().as_str(),
        }))
    );
    delete_entities(&broker, &[&entity.id]).await;
    Ok(())
}
