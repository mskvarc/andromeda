//! A `GeoProperty` `observationSpace` holding a Point is appended to a Building that holds no attribute (clauses 5.6.3
//! and 4.7).

use crate::{
    building_payloads::bare_building,
    entity_operations::{append_attributes, create_entity, delete_entities, retrieve_entity_json},
    geometry_payloads::point,
};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace, vocabulary::Vocabulary};
use googletest::prelude::*;
use ngsi_ld::{
    http::operation::Overwrite,
    model::{EntityFragment, NgsiLdMediaType},
};
use serde_json::json;

/// The terms of the `@context` the case registers.
const TERMS: [&str; 1] = ["Building"];

/// Pins: Check that one can append an observationSpace geospatial Property to an entity
/// Clause: 5.6.3, 4.7
#[gtest]
#[tokio::test]
async fn e010_08_append_observation_space_geospatial_property_to_an_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    let original = bare_building(&namespace, 1)?;
    create_entity(&broker, &vocabulary, &original).await?;
    let fragment = EntityFragment::builder().geo_property("observationSpace", point(13.3986, 52.5547)?).build()?;
    append_attributes(&broker, &original.id, &fragment, Overwrite::Replace).await?;
    let client = broker.with_context(vocabulary.context())?.with_accept(NgsiLdMediaType::Json);
    let body = retrieve_entity_json(&client, &original.id).await?;
    expect_that!(
        &body,
        matches_document(json!({
            "id": original.id.to_string(),
            "type": "Building",
            "observationSpace": {
                "type": "GeoProperty",
                "value": {"type": "Point", "coordinates": [13.3986, 52.5547]},
            },
        }))
    );
    delete_entities(&broker, &[&original.id]).await;
    Ok(())
}
