//! The Vehicle documents the batch tests expect a broker to forward to a context source or to answer with, described
//! member by member.

use andromeda_test_suite::namespace::Namespace;
use serde_json::{Value, json};

/// The Vehicle `urn:ngsi-ld:Vehicle:<ns>-<n>` holding only the `speed` of 56 from the Speedometer in the dataset
/// `urn:ngsi-ld:Property:<ns>-speedometerA4567-speed2`, with no `@context`.
pub(crate) fn speed_forwarded(namespace: &Namespace, n: u32) -> Value {
    json!({
        "id": namespace.urn_text("Vehicle", n),
        "type": "Vehicle",
        "speed": {
            "type": "Property",
            "value": 56,
            "source": {"type": "Property", "value": "Speedometer"},
            "datasetId": namespace.urn_text("Property", "speedometerA4567-speed2"),
        },
    })
}

/// The Vehicle `urn:ngsi-ld:Vehicle:<ns>-<n>`: a BMW whose `isParked2` Relationship is the `OffStreetParking`
/// Downtown2, observed at 2017-07-29T12:00:04Z, provided by Alice, with no `@context`.
pub(crate) fn parked2_forwarded(namespace: &Namespace, n: u32) -> Value {
    json!({
        "id": namespace.urn_text("Vehicle", n),
        "type": "Vehicle",
        "brandName": {"type": "Property", "value": "BMW"},
        "isParked2": {
            "type": "Relationship",
            "object": namespace.urn_text("OffStreetParking", "Downtown2"),
            "observedAt": "2017-07-29T12:00:04Z",
            "providedBy": {"type": "Relationship", "object": namespace.urn_text("Person", "Alice")},
        },
    })
}
