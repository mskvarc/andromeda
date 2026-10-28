//! ETSI `021_25`: the temporal evolutions of a private tenant are queried with `local=true`, the only narrowing of
//! the query, so the answer holds every temporal evolution of the tenant (clause 5.7.4).

use crate::suite_keywords::{create_vehicle_pair, delete_vehicle_pair};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace, problem::answered, wire::json_body};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    query::{Locality, temporal_query},
};
use serde_json::json;

/// Pins: Check that one can query all temporal evolution of entities if query is local
/// Clause: 5.7.4
/// Changed: observedAt is compared as written, not as the fractional-zero forms the expectation's regex admits
/// Corrected: `fix(021_25)`: expect both local entities from the unfiltered temporal query (5.7.4)
/// Tenant: private (local=true is the only narrowing, so the query answers every temporal entity of the tenant)
#[gtest]
#[tokio::test]
async fn e021_25_query_all_temporal_entities_with_local_flag() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let tenant = broker::private_tenant(&broker, &namespace).await?;
    let vocabulary = create_vehicle_pair(&tenant, &namespace, "021-06").await?;

    let received = tenant
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .temporal()
        .query(temporal_query::after("2020-07-01T12:05:00Z".parse()?))
        .locality(Locality::Local)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(
        &body,
        matches_document(json!([
            {
                "id": namespace.urn_text("Vehicle", "021-06-A"),
                "type": "Vehicle",
                "speed": [
                    {"type": "Property", "value": 120, "observedAt": "2020-08-01T12:03:00Z"},
                    {"type": "Property", "value": 80, "observedAt": "2020-08-01T12:05:00Z"},
                    {"type": "Property", "value": 100, "observedAt": "2020-08-01T12:07:00Z"},
                ],
                "fuelLevel": [
                    {"type": "Property", "value": 67, "observedAt": "2020-08-01T12:03:00Z"},
                    {"type": "Property", "value": 53, "observedAt": "2020-08-01T13:05:00Z"},
                    {"type": "Property", "value": 40, "observedAt": "2020-08-01T14:07:00Z"},
                ],
            },
            {
                "id": namespace.urn_text("Vehicle", "021-06-B"),
                "type": "Vehicle",
                "speed": [
                    {"type": "Property", "value": 120, "observedAt": "2020-09-01T12:03:00Z"},
                    {"type": "Property", "value": 80, "observedAt": "2020-09-01T12:05:00Z"},
                    {"type": "Property", "value": 100, "observedAt": "2020-09-01T12:07:00Z"},
                ],
                "fuelLevel": [
                    {"type": "Property", "value": 67, "observedAt": "2020-09-01T12:03:00Z"},
                    {"type": "Property", "value": 53, "observedAt": "2020-09-01T13:05:00Z"},
                    {"type": "Property", "value": 40, "observedAt": "2020-09-01T14:07:00Z"},
                ],
            },
        ]))
        .hiding("instanceId")
    );

    delete_vehicle_pair(&tenant, &namespace, "021-06").await?;
    Ok(())
}
