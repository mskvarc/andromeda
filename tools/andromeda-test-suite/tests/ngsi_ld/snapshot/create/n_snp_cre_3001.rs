//! A snapshot query breaking its data type restriction is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, written_json::json_request_to};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::model::ErrorType;

/// Pins: Create Snapshot refuses a `snapshotQueries` or `snapshotTemporalQueries` entry that is a Query breaking
/// the restrictions of its data type (an empty entities array, an empty pick array, an ordering member without
/// `orderBy`) with `BadRequestData`
/// Clause: 5.2.23, 5.2.43, 5.16.1, 5.5.4
#[gtest]
#[tokio::test]
async fn n_snp_cre_3001_a_snapshot_query_breaking_its_data_type_restriction_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_type = namespace.entity_type("Building")?;
    // Each body breaks a restriction of the Query data type that the typed query refuses, so it is written as text.
    let bodies = [
        format!(r#"{{"id": "urn:ngsi-ld:Snapshot:{namespace}-1", "type": "Snapshot", "snapshotQueries": [{{"type": "Query", "entities": []}}]}}"#),
        format!(
            r#"{{"id": "urn:ngsi-ld:Snapshot:{namespace}-2", "type": "Snapshot", "snapshotQueries": [{{"type": "Query", "entities": [{{"type": "{building_type}"}}], "pick": []}}]}}"#
        ),
        format!(
            r#"{{"id": "urn:ngsi-ld:Snapshot:{namespace}-3", "type": "Snapshot", "snapshotQueries": [{{"type": "Query", "entities": [{{"type": "{building_type}"}}], "ordering": {{"collation": "und-x-icu"}}}}]}}"#
        ),
        format!(
            r#"{{"id": "urn:ngsi-ld:Snapshot:{namespace}-4", "type": "Snapshot", "snapshotTemporalQueries": [{{"type": "Query", "entities": [], "temporalQ": {{"timerel": "after", "timeAt": "2000-01-01T00:00:00Z"}}}}]}}"#
        ),
    ];
    for body in &bodies {
        let request = json_request_to(&broker, Method::POST, "snapshots", body)?.append_header(ACCEPT, HeaderValue::from_static("application/json"));
        let answer = broker.send_unchecked(request).await;
        expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    }
    Ok(())
}
