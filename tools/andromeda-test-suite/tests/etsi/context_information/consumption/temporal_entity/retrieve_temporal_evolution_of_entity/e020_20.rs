//! The simplified temporal evolution of a scope is retrieved after the scope attribute is deleted, as the `deletedAt`
//! temporal property names the instance that the deletion recorded (clauses 4.18 and 5.7.3).

use crate::{entity_operations::delete_attribute, temporal_operations::deleted_temporal_values, vehicle_fixtures::create_vehicle_scoped_fuel};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace, teardown::AnyAnswer};
use googletest::prelude::*;
use serde_json::json;

/// Pins: Check that one can retrieve the temporal evolution of a scope following the deletedAt temporal property with
/// temporal values
/// Clause: 4.18, 5.7.3, 6.19.3.1
/// Expected: a deleted scope renders as urn:ngsi-ld:null (clauses 4.5.7, 4.18)
/// Expected: the deleted value is the NGSI-LD Null and comes first in the pair; a deletion instant depends on the run
/// that recorded it, so no fixed instant is compared
#[gtest]
#[tokio::test]
async fn e020_20_01_with_a_scope() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_scoped_fuel(&broker, &namespace).await?;

    let id = namespace.urn("Vehicle", 1)?;
    let client = broker.with_context(vocabulary.context())?;
    delete_attribute(&client, &id, "scope").await?;

    let body = deleted_temporal_values(&client, &id).await?;
    // The second item is the instant the deletion recorded, which the run fixes, so the expectation takes it from the
    // answer.
    let recorded = body.pointer("/scope/values/0/1");
    expect_that!(
        &body,
        matches_document(json!({
            "id": id.to_string(),
            "type": "Vehicle",
            "scope": {"type": "Property", "values": [["urn:ngsi-ld:null", recorded]]},
        }))
        .hiding("instanceId")
    );

    broker.temporal().delete(&id).send().await.any_answer();
    Ok(())
}
