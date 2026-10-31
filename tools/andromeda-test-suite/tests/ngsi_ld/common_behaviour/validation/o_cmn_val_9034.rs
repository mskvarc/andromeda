//! A refused path segment is named by its specification parameter.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, wire::request_to};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::model::ErrorType;

/// Pins: A path segment that is not a value of its parameter's type answers 400 `BadRequestData` whose detail names
/// the resource template's parameter (`entityId`, `attrId`, `instanceId`, type, `subscriptionId`) and the rule broken,
/// not the segment
/// Clause: 5.5.4, 6.5, 6.7, 6.22, 6.26, 6.11
/// Recorded: the specification types each path parameter but leaves the wording of a refusal open; Andromeda names the
/// parameter as the clause 6 resource template spells it
#[gtest]
#[tokio::test]
async fn o_cmn_val_9034_a_refused_path_segment_is_named_by_its_specification_parameter() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    // Each path names a segment the model refuses, so the requests are sent as written.
    let request =
        request_to(&broker, Method::GET, &format!("entities/bare-name-{namespace}"))?.append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer = broker.send_unchecked(request).await;
    expect_that!(
        &answer,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail("invalid path parameter entityId: invalid URI: not a valid URN, URL or IRI")
    );
    let request_2 = request_to(&broker, Method::DELETE, &format!("entities/urn:ngsi-ld:Vehicle:{namespace}-1/attrs/%20"))?;
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(
        &answer_2,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData)
            .with_detail("invalid path parameter attrId: a name must be a term, a compact IRI or an absolute IRI")
    );
    let request_3 = request_to(
        &broker,
        Method::DELETE,
        &format!("temporal/entities/urn:ngsi-ld:Vehicle:{namespace}-1/attrs/speed/bare-{namespace}"),
    )?;
    let answer_3 = broker.send_unchecked(request_3).await;
    expect_that!(
        &answer_3,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail("invalid path parameter instanceId: invalid URI: not a valid URN, URL or IRI")
    );
    let request_4 = request_to(&broker, Method::GET, "types/%20")?.append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_4 = broker.send_unchecked(request_4).await;
    expect_that!(
        &answer_4,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData)
            .with_detail("invalid path parameter type: a name must be a term, a compact IRI or an absolute IRI")
    );
    let request_5 =
        request_to(&broker, Method::GET, &format!("subscriptions/bare-{namespace}"))?.append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_5 = broker.send_unchecked(request_5).await;
    expect_that!(
        &answer_5,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData)
            .with_detail("invalid path parameter subscriptionId: invalid URI: not a valid URN, URL or IRI")
    );
    Ok(())
}
