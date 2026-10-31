//! A system attribute alone narrows only a registration discovery.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, wire::request_to, written_temporal_query::get_parameters};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::model::ErrorType;

/// Pins: A temporal query whose attrs or q names only system Attributes answers 400 `BadRequestData` (too wide) and
/// answers 200 once attrs also names a non-system Attribute, while the same attrs or q narrows a Query Context
/// Source Registrations, which answers 200
/// Clause: 5.7.4.4, 5.10.2.4, 4.8
/// Tenant: private (its queries select by attributes, q or geometry alone, so the broker reads every entity and
/// forwards to every registration of the tenant)
#[gtest]
#[tokio::test]
async fn n_tmp_qry_9007_a_system_attribute_alone_narrows_only_a_registration_discovery() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::private_tenant(&broker::front()?, &namespace).await?;
    // System attributes name no attribute the model accepts, so the requests naming them are sent as written.
    let time = [("timerel", "after"), ("timeAt", "2000-01-01T00:00:00Z")];
    for narrowing in [
        ("attrs", "createdAt"),
        ("attrs", "modifiedAt,deletedAt"),
        ("q", "createdAt>2020-01-01T00:00:00Z"),
    ] {
        let parameters = [time.as_slice(), &[narrowing]].concat();
        let answer = get_parameters(&broker, &parameters, "application/json").await?;
        expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    }
    for query in ["attrs=createdAt", "attrs=modifiedAt%2CdeletedAt", "q=createdAt%3E2020-01-01T00%3A00%3A00Z"] {
        let request =
            request_to(&broker, Method::GET, &format!("csourceRegistrations?{query}"))?.append_header(ACCEPT, HeaderValue::from_static("application/json"));
        let discovered = broker.send_unchecked(request).await?;
        expect_that!(discovered.status(), eq(StatusCode::OK));
    }
    let narrowing = format!("createdAt,speed_{namespace}");
    let parameters = [time.as_slice(), &[("attrs", narrowing.as_str())]].concat();
    let narrowed = get_parameters(&broker, &parameters, "application/json").await?;
    expect_that!(narrowed.map(|response| response.status()).ok(), some(eq(StatusCode::OK)));
    Ok(())
}
