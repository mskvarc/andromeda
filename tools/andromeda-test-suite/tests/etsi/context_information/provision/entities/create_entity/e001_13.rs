//! An entity with an `operationSpace` `GeoProperty` polygon is created and retrieved with it (clauses 5.6.1 and 4.7).

use crate::{
    entity_operations::{create_entity_answer, delete_entities, expect_created, retrieve_entity_json},
    geometry_payloads::polygon,
};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace, vocabulary::Vocabulary};
use googletest::prelude::*;
use ngsi_ld::model::{Entity, Input, NgsiLdMediaType};
use serde_json::json;

/// The Building `urn:ngsi-ld:Building:<ns>-1` whose `operationSpace` is the closed square ring of the corners
/// 100.12 to 101.12 longitude and 0.23 to 1.23 latitude.
fn building_operation_space(namespace: &Namespace) -> Result<Entity<Input>> {
    Ok(Entity::builder()
        .id(namespace.urn("Building", 1)?)
        .entity_type("Building")
        .geo_property(
            "operationSpace",
            polygon(&[(100.12, 0.23), (101.12, 0.23), (101.12, 1.23), (100.12, 1.23), (100.12, 0.23)])?,
        )
        .build()?)
}

/// Pins: Check that one can create an entity with operationSpace geospatial Property
/// Clause: 5.6.1, 4.7
#[gtest]
#[tokio::test]
async fn e001_13_operation_space_geoproperty() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building"]).await?;
    let entity = building_operation_space(&namespace)?;
    let created = create_entity_answer(&broker, &vocabulary, &entity).await?;
    expect_created(&created, &entity.id);
    let client = broker.with_context(vocabulary.context())?.with_accept(NgsiLdMediaType::JsonLd);
    let body = retrieve_entity_json(&client, &entity.id).await?;
    expect_that!(
        &body,
        matches_document(json!({
            "id": entity.id.to_string(),
            "type": "Building",
            "operationSpace": {
                "type": "GeoProperty",
                "value": {
                    "type": "Polygon",
                    "coordinates": [[[100.12, 0.23], [101.12, 0.23], [101.12, 1.23], [100.12, 1.23], [100.12, 0.23]]]
                }
            },
            "@context": vocabulary.url().as_str(),
        }))
    );
    delete_entities(&broker, &[&entity.id]).await;
    Ok(())
}
