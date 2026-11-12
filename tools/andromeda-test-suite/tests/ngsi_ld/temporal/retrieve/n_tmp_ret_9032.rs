//! A removed scope reads back as the null beside its deleted at.

use andromeda_test_suite::{body_json::json_absent, broker, namespace::Namespace, versioned_answer::send_versioned, written_json::json_request_to};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::{
        headers::Accept,
        operation::{NormalizedTemporal, RetrieveTemporalEvolution, TemporalSelection},
    },
    model::{NgsiLdMediaType, evolution::ScopeValue},
    query::temporal_query::{self, TemporalQuery},
};

/// Pins: Retrieve Temporal Evolution of an Entity whose scope was set at 12:10 and removed at 12:40 answers the removal
/// holding the NGSI-LD Null with a deletedAt, and no scope instance with an instanceId or a datasetId
/// Clause: 4.18, 4.5.6, 4.5.7
#[gtest]
#[tokio::test]
async fn n_tmp_ret_9032_a_removed_scope_reads_back_as_the_null_beside_its_deleted_at() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_removed_id = namespace.urn("Vehicle", "removed")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    // The NGSI-LD Null is not a scope a request can hold, so the Evolution is sent as written.
    let body = format!(
        r#"{{"id": "{vehicle_removed_id}", "type": "{vehicle_type}", "scope": [{{"type": "Property", "value": "/Madrid/Centro", "observedAt": "2018-08-01T12:10:00Z"}}, {{"type": "Property", "value": "urn:ngsi-ld:null", "observedAt": "2018-08-01T12:40:00Z"}}], "speed": [{{"type": "Property", "value": 30, "observedAt": "2018-08-01T12:20:00Z"}}]}}"#
    );
    let request = json_request_to(&broker, Method::POST, "temporal/entities", &body)?;
    let created = broker.send_unchecked(request).await?;
    expect_that!(created.status(), eq(StatusCode::CREATED));
    let client = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let retrieved = client
        .temporal()
        .retrieve(&vehicle_removed_id)
        .temporal_q(temporal_query::between("2018-08-01T12:00:00Z".parse()?, "2018-08-01T13:00:00Z".parse()?))
        .send()
        .await?;
    let removal = retrieved
        .body()
        .scope
        .iter()
        .find(|instance| instance.value() == &ScopeValue::Removed)
        .or_fail()?;
    expect_that!(removal.output().deleted_at, some(anything()));
    // A scope instance of the model holds no `instanceId` or `datasetId`, so their absence is read as received.
    let window = TemporalQuery::between("2018-08-01T12:00:00Z".parse()?, "2018-08-01T13:00:00Z".parse()?);
    let operation = RetrieveTemporalEvolution::builder()
        .id(&vehicle_removed_id)
        .selection(TemporalSelection::Query(&window))
        .format(NormalizedTemporal)
        .build();
    let answer = send_versioned(&client, &operation, None).await?;
    expect_that!(answer.status, eq(StatusCode::OK));
    expect_that!(&answer.body, json_absent("$.scope[*].instanceId"));
    expect_that!(&answer.body, json_absent("$.scope[*].datasetId"));
    Ok(())
}
