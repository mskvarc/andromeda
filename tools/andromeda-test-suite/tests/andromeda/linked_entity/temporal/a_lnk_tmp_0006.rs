//! A query body join follows the in window relationship instances.

use andromeda_test_suite::{
    attribute_matcher::property,
    broker,
    evolution_matcher::has_evolution,
    identity_matcher::has_id,
    linked_temporal_fixtures::ParkedVehicles,
    namespace::Namespace,
    nth::nth,
    problem::answered,
    temporal_page_body::entities,
    written_json::json_request_to,
};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};

/// Pins: Query Temporal Evolution of Entities by POST carries join and `joinLevel` in its Query body and answers
/// the root and the entity its in-window Relationship instances name, not the one named only outside the window
/// Clause: 4.5.23, 5.7.4, 6.24.3.1
/// Recorded: Andromeda extension, not v1.9.1: clauses 5.7.3.4 and 5.7.4.4 admit no Linked Entity retrieval on the
/// temporal operations; the `temporal_linked_retrieval` extension admits it under the Core rules of clause 4.5.23,
/// over the window of the request
#[gtest]
#[tokio::test]
async fn a_lnk_tmp_0006_a_query_body_join_follows_the_in_window_relationship_instances() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicles = ParkedVehicles::new(&namespace)?;
    vehicles.record_speeds(&broker, &vehicles.b, 30, 40).await?;
    vehicles.record_speeds(&broker, &vehicles.c, 30, 40).await?;
    vehicles.record_a(&broker).await?;
    // The Query document carries `join` and `joinLevel`, members the temporal query model does not write.
    let query = format!(
        r#"{{"type":"Query","entities":[{{"id":"{}","type":"{}"}}],"join":"flat","joinLevel":1,"temporalQ":{{"timerel":"between","timeAt":"2020-08-01T12:00:00Z","endTimeAt":"2020-08-01T12:10:00Z"}}}}"#,
        vehicles.a, vehicles.vehicle_type
    );
    let request =
        json_request_to(&broker, Method::POST, "temporal/entityOperations/query", &query)?.append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::OK));
    let page = entities(&answer?)?;
    expect_that!(page, len(eq(2)));
    expect_that!(page, nth(0, has_id(&vehicles.a)));
    expect_that!(
        page,
        nth(
            1,
            all!(
                has_id(&vehicles.b),
                has_evolution("speed", len(eq(2))),
                has_evolution("speed", nth(0, property().value(30)))
            )
        )
    );
    Ok(())
}
