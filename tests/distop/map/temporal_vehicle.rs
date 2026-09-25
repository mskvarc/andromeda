//! Temporal `Vehicle` fixture and base temporal query for the temporal-EntityMap
//! cases (clause 5.14.5). The fixture records three `speed` observations across
//! 2021 plus a located instant, so a temporal window can include or exclude it.

use andromeda_test_fixtures::context::COMPOUND_CONTEXT;
use serde_json::{Value, json};

/// The temporal query selecting all history before 2022.
pub const BEFORE_2022: &str = "timerel=before&timeAt=2022-01-01T00:00:00Z";

/// A temporal entity of the given type with three `speed` observations in 2021
/// and a located instant at the origin, for suites that derive a per-test type
/// so a bare `type=` temporal query cannot see another concurrent test's
/// entities.
pub fn temporal_vehicle_typed(id: &str, entity_type: &str, speed: i64) -> Value {
    json!({
        "id": id,
        "type": entity_type,
        "speed": [
            { "type": "Property", "value": speed, "observedAt": "2021-01-01T00:00:00Z" },
            { "type": "Property", "value": speed + 1, "observedAt": "2021-06-01T00:00:00Z" },
            { "type": "Property", "value": speed + 2, "observedAt": "2021-12-01T00:00:00Z" }
        ],
        "location": {
            "type": "GeoProperty",
            "value": { "type": "Point", "coordinates": [0, 0] },
            "observedAt": "2021-01-01T00:00:00Z"
        },
        "@context": COMPOUND_CONTEXT.as_str()
    })
}
