//! The temporal evolution of a Vehicle whose `isParkedIn` Relationship names two objects is queried in the normalized
//! representation and in the simplified temporal representation (clauses 5.7.4 and 4.5.9).

use crate::{
    temporal_operations::{create_temporal_evolution, delete_temporal_entities},
    vehicle_evolution_payloads::vehicle_parked_in_two,
};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace, problem::answered, wire::json_body};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::RepresentationSpelling},
    query::temporal_query,
};
use serde_json::json;

/// Pins: Check that one can query the temporal evolution of entities with a multivalued relationship
/// Clause: 5.7.4
#[gtest]
#[tokio::test]
async fn e021_26_01_query_the_temporal_evolution_of_entities_with_a_multivalued_relationship() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_temporal_evolution(&broker, &namespace, &["Vehicle", "isParkedIn"], &vehicle_parked_in_two(&namespace)?).await?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .temporal()
        .query(temporal_query::after("2020-07-01T12:05:00Z".parse()?))
        .entity_type("Vehicle")
        .ids(vec![namespace.urn("Vehicle", "021-26")?])
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(
        &body,
        matches_document(json!([
            {
                "id": namespace.urn_text("Vehicle", "021-26"),
                "type": "Vehicle",
                "isParkedIn": [
                    {
                        "type": "Relationship",
                        "object": [
                            namespace.urn_text("OffStreetParking", "Downtown1"),
                            namespace.urn_text("OffStreetParking", "Downtown2"),
                        ],
                        "observedAt": "2020-08-01T12:03:00Z",
                    },
                ],
            },
        ]))
        .hiding("instanceId")
    );

    delete_temporal_entities(&broker, &[&namespace.urn("Vehicle", "021-26")?]).await;
    Ok(())
}

/// Pins: Check that one can query the temporal evolution of entities with a multivalued relationship
/// Clause: 5.7.4, 4.5.9
#[gtest]
#[tokio::test]
async fn e021_26_02_query_the_simplified_temporal_evolution_of_entities_with_a_multivalued_relationship() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_temporal_evolution(&broker, &namespace, &["Vehicle", "isParkedIn"], &vehicle_parked_in_two(&namespace)?).await?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .temporal()
        .query_values(temporal_query::after("2020-07-01T12:05:00Z".parse()?))
        .entity_type("Vehicle")
        .ids(vec![namespace.urn("Vehicle", "021-26")?])
        .spelling(RepresentationSpelling::default().in_options())
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(
        &body,
        matches_document(json!([
            {
                "id": namespace.urn_text("Vehicle", "021-26"),
                "type": "Vehicle",
                "isParkedIn": {
                    "type": "Relationship",
                    "objects": [
                        [
                            [
                                namespace.urn_text("OffStreetParking", "Downtown1"),
                                namespace.urn_text("OffStreetParking", "Downtown2"),
                            ],
                            "2020-08-01T12:03:00Z",
                        ],
                    ],
                },
            },
        ]))
        .hiding("instanceId")
    );

    delete_temporal_entities(&broker, &[&namespace.urn("Vehicle", "021-26")?]).await;
    Ok(())
}
