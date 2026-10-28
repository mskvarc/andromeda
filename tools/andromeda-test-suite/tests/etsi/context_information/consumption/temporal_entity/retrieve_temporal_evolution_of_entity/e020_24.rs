//! ETSI `020_24`: the temporal evolution of an entity whose relationship has several objects is retrieved, in
//! normalized and in simplified temporal representation (clauses 5.7.3 and 4.5.9).

use crate::suite_keywords::create_temporal_evolution;
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace, problem::answered, teardown::AnyAnswer, wire::json_body};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::RepresentationSpelling},
    model::{Input, Relationship, evolution::TemporalEntity},
};
use serde_json::json;

/// The Vehicle `urn:ngsi-ld:Vehicle:<ns>-1` of the cases: an `isParkedIn` Relationship observed on 2020-08-01 at
/// 12:03, whose objects are the `OffStreetParking` Downtown1 and Downtown2, in UTC.
fn vehicle_parked_in_two_parkings(namespace: &Namespace) -> Result<TemporalEntity<Input>> {
    let parkings = vec![namespace.urn("OffStreetParking", "Downtown1")?, namespace.urn("OffStreetParking", "Downtown2")?];
    Ok(TemporalEntity::builder()
        .id(namespace.urn("Vehicle", 1)?)
        .entity_type("Vehicle")
        .relationship("isParkedIn", Relationship::builder(parkings).observed_at("2020-08-01T12:03:00Z".parse()?))
        .build()?)
}

/// Pins: Check that one can retrieve the temporal evolution of an entity with a multivalued relationship
/// Clause: 5.7.3
#[gtest]
#[tokio::test]
async fn e020_24_01_retrieve_the_temporal_evolution_of_an_entity_with_a_multivalued_relationship() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_temporal_evolution(&broker, &namespace, &["Vehicle", "isParkedIn"], &vehicle_parked_in_two_parkings(&namespace)?).await?;

    let id = namespace.urn("Vehicle", 1)?;
    let downtown1 = namespace.urn("OffStreetParking", "Downtown1")?.to_string();
    let downtown2 = namespace.urn("OffStreetParking", "Downtown2")?.to_string();
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .temporal()
        .retrieve(&id)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(
        &body,
        matches_document(json!({
            "id": id.to_string(),
            "type": "Vehicle",
            "isParkedIn": [{
                "type": "Relationship",
                "object": [downtown1, downtown2],
                "observedAt": "2020-08-01T12:03:00Z",
            }],
        }))
        .hiding("instanceId")
    );

    broker.temporal().delete(&id).send().await.any_answer();
    Ok(())
}

/// Pins: Check that one can retrieve the temporal evolution of an entity with a multivalued relationship
/// Clause: 5.7.3, 4.5.9
#[gtest]
#[tokio::test]
async fn e020_24_02_retrieve_the_simplified_temporal_evolution_of_an_entity_with_a_multivalued_relationship() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_temporal_evolution(&broker, &namespace, &["Vehicle", "isParkedIn"], &vehicle_parked_in_two_parkings(&namespace)?).await?;

    let id = namespace.urn("Vehicle", 1)?;
    let downtown1 = namespace.urn("OffStreetParking", "Downtown1")?.to_string();
    let downtown2 = namespace.urn("OffStreetParking", "Downtown2")?.to_string();
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .temporal()
        .retrieve_values(&id)
        .spelling(RepresentationSpelling::default().in_options())
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(
        &body,
        matches_document(json!({
            "id": id.to_string(),
            "type": "Vehicle",
            "isParkedIn": {
                "type": "Relationship",
                "objects": [[[downtown1, downtown2], "2020-08-01T12:03:00Z"]],
            },
        }))
        .hiding("instanceId")
    );

    broker.temporal().delete(&id).send().await.any_answer();
    Ok(())
}
