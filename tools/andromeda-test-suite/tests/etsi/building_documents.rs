//! The Buildings and `GeoJSON` collections as the broker answers them, which the query tests compare their answers
//! with.

use andromeda_test_suite::namespace::Namespace;
use serde_json::{Value, json};

/// The `GeoJSON` collection of `features`, with the vocabulary `context` on the collection when `top` is set and on
/// each feature when `on_features` is set: the placements a `GeoJSON` answer under a `Link` context may take.
pub(crate) fn feature_collection(features: [Value; 2], context: &str, top: bool, on_features: bool) -> Value {
    let features = if on_features {
        features.map(|feature| with_feature_context(feature, context))
    } else {
        features
    };
    let mut collection = json!({"type": "FeatureCollection", "features": features});
    if top {
        collection["@context"] = json!(context);
    }
    collection
}

/// Adds the vocabulary `context` to a feature.
fn with_feature_context(mut feature: Value, context: &str) -> Value {
    feature["@context"] = json!(context);
    feature
}

/// The Building `urn:ngsi-ld:Building:<ns>-<n>` as the broker retrieves it after [`building_simple_attributes`].
pub(crate) fn simple_building_json(namespace: &Namespace, n: u32, context: &str) -> Value {
    json!({
        "id": namespace.urn_text("Building", n),
        "type": "Building",
        "name": {"type": "Property", "value": "Eiffel Tower"},
        "subCategory": {"type": "Property", "value": "tourism"},
        "airQualityLevel": {
            "type": "Property",
            "value": 4,
            "unitCode": "C62",
            "observedAt": "2020-09-09T16:40:00.000Z"
        },
        "almostFull": {"type": "Property", "value": false},
        "@context": context,
    })
}

/// The Building `urn:ngsi-ld:Building:<ns>-<n>` as the broker retrieves it after [`building_with_weather_sensor`].
pub(crate) fn weather_building_json(namespace: &Namespace, n: u32, context: &str) -> Value {
    json!({
        "id": namespace.urn_text("Building", n),
        "type": "Building",
        "name": {"type": "Property", "value": "Eiffel Tower"},
        "subCategory": {"type": "Property", "value": "tourism"},
        "airQualityLevel": {
            "type": "Property",
            "value": 4,
            "unitCode": "C62",
            "observedAt": "2020-09-09T16:40:00.000Z",
            "hasWeatherObserved": {
                "type": "Relationship",
                "object": namespace.urn_text("Sensor", "43a1af27-4aa7-44fe-bee9-fe76d56978a4")
            }
        },
        "almostFull": {"type": "Property", "value": false},
        "@context": context,
    })
}

/// The Building `urn:ngsi-ld:Building:<ns>-<n>` of [`simple_building_json`] with the `locatedAt` Relationship to the
/// City Paris that a batch writes.
pub(crate) fn located_building_json(namespace: &Namespace, n: u32, context: &str) -> Value {
    let mut building = simple_building_json(namespace, n, context);
    building["locatedAt"] = json!({"type": "Relationship", "object": namespace.urn_text("City", "Paris")});
    building
}

/// The Building `urn:ngsi-ld:Building:<ns>-<n>` with only its `id`, `type` and `@context`, as the broker retrieves it
/// after an entity of that shape is created.
pub(crate) fn bare_building_json(namespace: &Namespace, n: u32, context: &str) -> Value {
    json!({
        "id": namespace.urn_text("Building", n),
        "type": "Building",
        "@context": context,
    })
}

/// The Building `urn:ngsi-ld:Building:<ns>-<n>` with only its `id`, `type`, `@context` and the `locatedAt`
/// Relationship to the City Paris, as the broker retrieves it after the Paris Building of the linked cases is written.
pub(crate) fn bare_located_building_json(namespace: &Namespace, n: u32, context: &str) -> Value {
    let mut building = bare_building_json(namespace, n, context);
    building["locatedAt"] = json!({"type": "Relationship", "object": namespace.urn_text("City", "Paris")});
    building
}

/// The entity `urn:ngsi-ld:Building:<ns>-<n>` with only its `id`, `type` (the `types` in the order given) and
/// `@context`.
pub(crate) fn typed_building_json(namespace: &Namespace, n: u32, types: &[&str], context: &str) -> Value {
    json!({
        "id": namespace.urn_text("Building", n),
        "type": types,
        "@context": context,
    })
}
