//! The Batch Entity Merge the merge cases send, of the Vehicles `-1` and `-2` as
//! [`vehicle_speed_parked2_by_bob`] writes them, and the merged Vehicles a source answers a query with.

use crate::{distributed_operations::vehicle_payloads::vehicle_speed_parked2_by_bob, entity_operations::ld_json_body_client};
use andromeda_test_suite::{broker::BrokerClient, namespace::Namespace, problem::answered, vocabulary::Vocabulary};
use googletest::prelude::*;
use http::StatusCode;
use serde_json::{Value, json};

/// `Batch Merge Entities` of the fragments of the Vehicles `-1` and `-2`, as `application/ld+json` with the vocabulary
/// as a one-element `@context` list in each fragment: requires 204.
pub(crate) async fn merge_vehicles(broker: &BrokerClient, vocabulary: &Vocabulary, namespace: &Namespace) -> Result<()> {
    let fragments = [
        vehicle_speed_parked2_by_bob(namespace, 1)?.into_fragment(),
        vehicle_speed_parked2_by_bob(namespace, 2)?.into_fragment(),
    ];
    let answer = ld_json_body_client(broker, vocabulary)?
        .with_responses()
        .entities()
        .batch_merge(&fragments)
        .send()
        .await;
    verify_that!(&answer, answered(StatusCode::NO_CONTENT))
}

/// The merged Vehicles `-1` and `-2` a source answers a query with: the fragments' members and the vocabulary as a
/// one-element `@context` list.
pub(crate) fn merged_vehicles_reply(namespace: &Namespace, vocabulary: &Vocabulary) -> Value {
    let merged = |n: u32| {
        json!({
            "id": namespace.urn_text("Vehicle", n),
            "type": "Vehicle",
            "speed": {
                "type": "Property",
                "value": 56,
                "source": {"type": "Property", "value": "Speedometer"},
                "datasetId": namespace.urn_text("Property", "speedometerA4567-speed2"),
            },
            "isParked2": {
                "type": "Relationship",
                "object": namespace.urn_text("OffStreetParking", "Downtown2"),
                "observedAt": "2019-07-29T12:00:04Z",
                "providedBy": {"type": "Relationship", "object": namespace.urn_text("Person", "Bob")},
            },
            "@context": [vocabulary.url().as_str()],
        })
    };
    json!([merged(1), merged(2)])
}
