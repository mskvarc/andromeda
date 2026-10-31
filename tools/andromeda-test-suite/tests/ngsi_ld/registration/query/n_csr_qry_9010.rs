//! An end time at outside a between window is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, raw_call::target, wire::request_to};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType},
    query::temporal_query,
};

/// Pins: Query Context Source Registrations with timerel before or after and an `endTimeAt`, or a between whose
/// `endTimeAt` is not after its `timeAt`, answers 400 `BadRequestData` naming `endTimeAt`
/// Clause: 4.11, Table 6.8.3.2-1, 5.10.2.4, 5.5.4
#[gtest]
#[tokio::test]
async fn n_csr_qry_9010_an_end_time_at_outside_a_between_window_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_type = namespace.entity_type("Building")?;
    let type_text = building_type.to_string();
    // Each window breaks a rule the typed temporal query refuses, so the query string is written as given.
    let windows = [
        ("before", "2026-01-01T00:00:00Z", "2026-02-01T00:00:00Z"),
        ("after", "2026-01-01T00:00:00Z", "2026-02-01T00:00:00Z"),
        ("between", "2026-01-01T00:00:00Z", "2026-01-01T00:00:00Z"),
        ("between", "2026-02-01T00:00:00Z", "2026-01-01T00:00:00Z"),
    ];
    for (timerel, time_at, end_time_at) in windows {
        let path = target(
            "csourceRegistrations",
            &[
                ("type", type_text.as_str()),
                ("timerel", timerel),
                ("timeAt", time_at),
                ("endTimeAt", end_time_at),
            ],
        );
        let request = request_to(&broker, Method::GET, &path)?.append_header(ACCEPT, HeaderValue::from_static("application/json"));
        let answer = broker.send_unchecked(request).await;
        expect_that!(
            &answer,
            problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_body_containing("endTimeAt")
        );
    }
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .entity_type(&building_type)
        .temporal_q(temporal_query::between("2026-01-01T00:00:00Z".parse()?, "2026-02-01T00:00:00Z".parse()?))
        .send()
        .await?;
    expect_that!(page.items(), is_empty());
    Ok(())
}
