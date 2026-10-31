//! A tenant header naming no tenant is refused with its cause.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    problem::{answered, problem},
    request_fields::with_fields,
    wire::request_to,
};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{client::ClientError, model::ErrorType};

/// Pins: Query Entities with an empty NGSILD-Tenant header answers 400 `BadRequestData` saying the header is empty,
/// and with 1abc or bad tenant answers 400 `BadRequestData` saying the header is not a valid tenant name
/// Clause: 6.3.14, 6.3.2
/// Tenant: refused (the subject is an NGSILD-Tenant value that names no tenant)
/// Recorded: Andromeda bound, not v1.9.1: clause 6.3.14 makes the header a String; Andromeda keys a tenant by a name
/// matching `[A-Za-z_][A-Za-z0-9_-]*` of at most 1,005 octets, and refuses any other value naming why
#[gtest]
#[tokio::test]
async fn a_cmn_hdr_9009_a_tenant_header_naming_no_tenant_is_refused_with_its_cause() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    // The tenant header is a value of the client's own, so a value that names no tenant is written as given.
    let query = |tenant: &str| -> Result<_> {
        Ok(with_fields(
            request_to(&broker, Method::GET, &format!("entities?type={vehicle_type}"))?.append_header(ACCEPT, HeaderValue::from_static("application/json")),
            &[("NGSILD-Tenant", tenant)],
        )?)
    };
    let answer = broker.send_unchecked(query("")?).await;
    expect_that!(
        &answer,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail("the NGSILD-Tenant header is empty")
    );
    for tenant in ["1abc", "bad tenant"] {
        let answer_2 = broker.send_unchecked(query(tenant)?).await;
        expect_that!(&answer_2, answered(StatusCode::BAD_REQUEST));
        expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
        let detail = answer_2
            .as_ref()
            .err()
            .and_then(ClientError::problem)
            .and_then(|problem| problem.detail.as_deref());
        expect_that!(detail, some(starts_with("the NGSILD-Tenant header is not a valid tenant name")));
    }
    Ok(())
}
