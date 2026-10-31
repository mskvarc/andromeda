//! The temporal evolution of a scope is retrieved after the scope attribute is deleted, as the `deletedAt` temporal
//! property names the instance that the deletion recorded (clauses 4.18 and 5.7.3).

use crate::{entity_operations::delete_attribute, vehicle_fixtures::create_vehicle_scoped_fuel};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace, problem::answered, teardown::AnyAnswer, wire::json_body};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{http::headers::Accept, query::temporal_query::TemporalProperty};
use serde_json::json;

/// Pins: Check that one can retrieve the temporal evolution of a scope following the deletedAt temporal property
/// Clause: 4.18, 5.7.3, 6.19.3.1
/// Expected: a deleted scope renders as urn:ngsi-ld:null (clauses 4.5.7, 4.18)
#[gtest]
#[tokio::test]
async fn e020_19_01_with_a_scope() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_scoped_fuel(&broker, &namespace).await?;

    let id = namespace.urn("Vehicle", 1)?;
    let client = broker.with_context(vocabulary.context())?;
    delete_attribute(&client, &id, "scope").await?;

    let received = client
        .with_accept(Accept::any())
        .with_responses()
        .temporal()
        .retrieve(&id)
        .time_property(TemporalProperty::DeletedAt)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(
        &body,
        matches_document(json!({
            "id": id.to_string(),
            "type": "Vehicle",
            "scope": [{"type": "Property", "value": "urn:ngsi-ld:null"}],
        }))
        .hiding_everywhere("deletedAt")
        .hiding("instanceId")
    );
    expect_that!(body.pointer("/scope/0/deletedAt"), some(anything()));

    broker.temporal().delete(&id).send().await.any_answer();
    Ok(())
}
