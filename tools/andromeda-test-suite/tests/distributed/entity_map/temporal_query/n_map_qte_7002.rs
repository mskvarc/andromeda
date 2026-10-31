//! Linked entity retrieval in a projection or a query is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, raw_call::target, wire::request_to};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::model::ErrorType;

/// Pins: Create `EntityMap` for Query Temporal Evolution of Entities with a pick through a Linked Entity term, or a
/// q over a Linked Entity attribute, answers 400 `BadRequestData`
/// Clause: 5.14.5.4, 4.21, 4.9
#[gtest]
#[tokio::test]
async fn n_map_qte_7002_linked_entity_retrieval_in_a_projection_or_a_query_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_type = namespace.suffixed("Vehicle");
    for (name, value) in [("pick", "linkedTo{speed}"), ("q", "linkedTo{speed}>5")] {
        // The refusal is the subject, so the Linked Entity term goes out as the test writes it.
        let request = request_to(
            &broker,
            Method::GET,
            &target(
                "temporal/entityMaps",
                &[
                    ("type", vehicle_type.as_str()),
                    ("timerel", "before"),
                    ("timeAt", "2022-01-01T00:00:00Z"),
                    (name, value),
                ],
            ),
        )?
        .append_header(ACCEPT, HeaderValue::from_static("application/json"));
        let answer = broker.send_unchecked(request).await;
        expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    }
    Ok(())
}
