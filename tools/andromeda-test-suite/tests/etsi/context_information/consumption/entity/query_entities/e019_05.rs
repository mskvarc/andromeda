//! ETSI `019_05`: entities queried by id are returned as a `GeoJSON` collection (clause 6.3.7).
//!
//! The collection's `@context` may sit on the collection, on each feature, or on both, as the case allows.

use crate::{
    suite_keywords::{create_entity, delete_entities, feature_collection},
    suite_payloads::{BUILDING_SIMPLE_TERMS, eiffel_tower_located},
};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace, problem::answered, vocabulary::Vocabulary, wire::json_body};
use googletest::prelude::*;
use http::StatusCode;
use serde_json::{Value, json};

/// The Feature of the Building `urn:ngsi-ld:Building:<ns>-<suffix>`, named Eiffel Tower and located at 13.3986, 52.5547,
/// with compact property names as the case's vocabulary writes them.
fn building_feature(namespace: &Namespace, suffix: &str) -> Value {
    json!({
        "id": namespace.urn_text("Building", suffix),
        "type": "Feature",
        "geometry": {"type": "Point", "coordinates": [13.3986, 52.5547]},
        "properties": {
            "type": "Building",
            "name": {"type": "Property", "value": "Eiffel Tower"},
            "subCategory": {"type": "Property", "value": "tourism"},
            "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [13.3986, 52.5547]}},
        },
    })
}

/// Pins: Check that the queried entities by id can be returned in `GeoJSON` format
/// Clause: 6.3.7
/// Changed: the setup creations require 201, where the case accepts any answer
/// Changed: the response's `@context` may sit on the collection, on each feature, or on both, as the case allows
#[gtest]
#[tokio::test]
async fn e019_05_01_query_by_ids_in_geojson() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_SIMPLE_TERMS[..3]).await?;
    create_entity(&broker, &vocabulary, &eiffel_tower_located(&namespace, "019-05-01")?).await?;
    create_entity(&broker, &vocabulary, &eiffel_tower_located(&namespace, "019-05-02")?).await?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_responses()
        .entities()
        .query_geo_json()
        .ids(vec![namespace.urn("Building", "019-05-01")?, namespace.urn("Building", "019-05-02")?])
        .entity_type(namespace.expanded_term("Building")?.as_str())
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    let url = vocabulary.url().to_string();
    let features = || [building_feature(&namespace, "019-05-01"), building_feature(&namespace, "019-05-02")];
    expect_that!(
        &body,
        any!(
            matches_document(feature_collection(features(), &url, true, false)).hiding("instanceId"),
            matches_document(feature_collection(features(), &url, false, true)).hiding("instanceId"),
            matches_document(feature_collection(features(), &url, true, true)).hiding("instanceId"),
        )
    );
    delete_entities(&broker, &[&namespace.urn("Building", "019-05-01")?, &namespace.urn("Building", "019-05-02")?]).await;
    Ok(())
}
