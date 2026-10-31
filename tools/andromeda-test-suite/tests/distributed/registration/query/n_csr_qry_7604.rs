//! A malformed id q geo query or temporal query is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, raw_call::target, wire::request_to};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::model::ErrorType;

/// Pins: Query Context Source Registrations on broker A by type with an id that is not a URI, a malformed q, a
/// geometry without georel, or a timerel without `timeAt` answers 400 `BadRequestData`
/// Clause: 5.10.2.4, 4.9, 4.10, 4.11
#[gtest]
#[tokio::test]
async fn n_csr_qry_7604_a_malformed_id_q_geo_query_or_temporal_query_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let type_text = namespace.suffixed("Vehicle");
    // Each query breaks a parameter rule the parameter models enforce, so the requests are sent as written.
    for malformed in [("id", "not-a-uri"), ("q", "speed>>10"), ("geometry", "Point"), ("timerel", "after")] {
        let parameters: Vec<(&str, &str)> = match malformed {
            ("geometry", geometry) => vec![("type", type_text.as_str()), ("geometry", geometry), ("coordinates", "[1,2]")],
            (name, value) => vec![("type", type_text.as_str()), (name, value)],
        };
        let request = request_to(&broker, Method::GET, &target("csourceRegistrations", &parameters))?;
        let answer = broker.send_unchecked(request).await;
        expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    }
    Ok(())
}
