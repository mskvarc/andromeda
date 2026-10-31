//! A refused query parameter is named and its value is not quoted.

use andromeda_test_suite::{
    broker::{self, BrokerClient},
    namespace::Namespace,
    problem::problem,
    wire::{WireError, request_to},
};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::{UncheckedRequest, headers::Accept},
    model::{ErrorType, NgsiLdMediaType},
};

/// Pins: A query parameter value that does not decode answers 400 `BadRequestData` whose detail names the parameter and the rule broken without quoting the value (limit, count, join, also inside the flattened page group), a parameter the operation does not define answers 400 `InvalidRequest` naming it (limit on Retrieve Entity and on Purge Snapshots), and a name no operation defines answers 400 `InvalidRequest`
/// Clause: 5.5.4, 6.3.20, Table 6.4.3.2-1, Table 6.5.3.1-1, Table 6.36.3.2-1
/// Recorded: clause 6.3.20 fixes the error types but not the wording; Andromeda names the parameter as the specification spells it and never repeats the rejected value
#[gtest]
#[tokio::test]
async fn o_cmn_val_9035_a_refused_query_parameter_is_named_and_its_value_is_not_quoted() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?.with_accept(Accept::new(NgsiLdMediaType::Json));
    let vehicle_1_id = namespace.urn("Vehicle", 1)?;
    let snapshot_id = namespace.urn("Snapshot", 1)?;
    // Every request below carries a parameter value or name the client cannot write, so each is sent as written.
    let limit_answer = broker
        .send_unchecked(accepting(
            &broker,
            Method::GET,
            &format!("entities?type=Vehicle_{namespace}&limit=ten-{namespace}"),
        )?)
        .await;
    expect_that!(
        &limit_answer,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail("invalid query parameter limit: not a non-negative integer")
    );
    let count_answer = broker
        .send_unchecked(accepting(
            &broker,
            Method::GET,
            &format!("entities?type=Vehicle_{namespace}&count=yes-{namespace}"),
        )?)
        .await;
    expect_that!(
        &count_answer,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail("invalid query parameter count: provided string was not `true` or `false`")
    );
    let join_answer = broker
        .send_unchecked(accepting(
            &broker,
            Method::GET,
            &format!("entities?type=Vehicle_{namespace}&join=sideways-{namespace}"),
        )?)
        .await;
    expect_that!(
        &join_answer,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail("invalid query parameter join: not one of the values the parameter defines")
    );
    let retrieve_answer = broker
        .send_unchecked(accepting(&broker, Method::GET, &format!("entities/{vehicle_1_id}?limit=3"))?)
        .await;
    expect_that!(
        &retrieve_answer,
        problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest).with_detail("query parameter limit is not defined for this operation")
    );
    let unknown_answer = broker
        .send_unchecked(accepting(&broker, Method::GET, &format!("entities?type=Vehicle_{namespace}&limitt=3"))?)
        .await;
    expect_that!(
        &unknown_answer,
        problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest).with_detail("the query names a parameter no NGSI-LD operation defines")
    );
    let purge_answer = broker
        .send_unchecked(request_to(&broker, Method::DELETE, &format!("snapshots?q=id%3D%3D{snapshot_id}&limit=3"))?)
        .await;
    expect_that!(
        &purge_answer,
        problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest).with_detail("query parameter limit is not defined for this operation")
    );
    Ok(())
}

/// A request with `method` to `target` (relative to the API root, with its query) asking for `application/json`.
fn accepting(broker: &BrokerClient, method: Method, target: &str) -> Result<UncheckedRequest, WireError> {
    Ok(request_to(broker, method, target)?.append_header(ACCEPT, HeaderValue::from_static("application/json")))
}
