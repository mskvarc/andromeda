//! A refusal raised while reading the request names the request target.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    problem::{answered, instance_of},
    wire::{api_uri, request_to},
};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::LINK};
use ngsi_ld::{
    http::operation::{BatchUpsert, QueryEntities, QueryMethod},
    model::{Entity, Lossless},
    query::builders::SelectorTypeBuilder,
};

/// Pins: A malformed q, a bare-name id, a malformed Link header, an unknown query parameter, an undecodable query
/// body, an unknown parameter on a batch upsert and a method the snapshots collection does not allow are each
/// refused with a problem detail whose instance is the request target, query string included
/// Clause: 5.5.3, 6.3.2
/// Recorded: 5.5.3 and IETF RFC 7807 section 3.1 make instance "a URI reference that identifies the specific
/// occurrence of the problem" without saying which; Andromeda names the request target the client sent, path and
/// query string
#[gtest]
#[tokio::test]
async fn o_cmn_err_9001_a_refusal_raised_while_reading_the_request_names_the_request_target() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_type = namespace.entity_type("Building")?;
    let selector = SelectorTypeBuilder::from(&building_type).build()?;
    let query = QueryEntities::builder().entity_type(&selector).format(Lossless::Normalized).build();
    // The q model refuses the malformed query, so the target is written as given.
    let malformed_q = format!("entities?type={building_type}&q=bad((");
    let answer = broker.send_unchecked(request_to(&broker, Method::GET, &malformed_q)?).await;
    expect_that!(&answer, answered(StatusCode::BAD_REQUEST));
    let instance = format!("/ngsi-ld/v1/{malformed_q}");
    expect_that!(instance_of(&answer), some(eq(instance.as_str())));
    // The model refuses an entity id that is no URI, so the target is written as given.
    let answer_2 = broker.send_unchecked(request_to(&broker, Method::GET, "entities/bare-name")?).await;
    expect_that!(&answer_2, answered(StatusCode::BAD_REQUEST));
    expect_that!(instance_of(&answer_2), some(eq("/ngsi-ld/v1/entities/bare-name")));
    // The Link header is malformed, which no typed client writes, so it is added to the typed query.
    let request_3 = broker
        .unchecked(&query)?
        .append_header(LINK, HeaderValue::from_static("this is not a link header"));
    let answer_3 = broker.send_unchecked(request_3).await;
    expect_that!(&answer_3, answered(StatusCode::BAD_REQUEST));
    let instance_3 = format!("/ngsi-ld/v1/entities?type={building_type}");
    expect_that!(instance_of(&answer_3), some(eq(instance_3.as_str())));
    // Query Entities defines no `unknownParameter`, so the typed query is sent to the target as written.
    let unknown_parameter = format!("entities?type={building_type}&unknownParameter=1");
    let request_4 = broker.unchecked(&query)?.uri(api_uri(&broker, &unknown_parameter)?);
    let answer_4 = broker.send_unchecked(request_4).await;
    expect_that!(&answer_4, answered(StatusCode::BAD_REQUEST));
    let instance_4 = format!("/ngsi-ld/v1/{unknown_parameter}");
    expect_that!(instance_of(&answer_4), some(eq(instance_4.as_str())));
    // The body is not JSON, which no typed operation writes, so it replaces the body of the typed query.
    let posted = QueryEntities::builder()
        .entity_type(&selector)
        .method(QueryMethod::Post)
        .format(Lossless::Normalized)
        .build();
    let request_5 = broker.unchecked(&posted)?.body(Bytes::from_static(b"{"));
    let answer_5 = broker.send_unchecked(request_5).await;
    expect_that!(&answer_5, answered(StatusCode::BAD_REQUEST));
    expect_that!(instance_of(&answer_5), some(eq("/ngsi-ld/v1/entityOperations/query")));
    // The client refuses an empty batch and an unknown parameter, so a placeholder upsert goes to the target with `[]`.
    let placeholder = [Entity::builder()
        .id(namespace.urn("Building", "placeholder")?)
        .entity_type(building_type.clone())
        .build()?];
    let request_6 = broker
        .unchecked(&BatchUpsert::builder().entities(&placeholder).build())?
        .uri(api_uri(&broker, "entityOperations/upsert?unknownParameter=1")?)
        .body(Bytes::from_static(b"[]"));
    let answer_6 = broker.send_unchecked(request_6).await;
    expect_that!(&answer_6, answered(StatusCode::BAD_REQUEST));
    expect_that!(instance_of(&answer_6), some(eq("/ngsi-ld/v1/entityOperations/upsert?unknownParameter=1")));
    // No typed operation deletes the snapshots collection, so the request is written as given.
    let answer_7 = broker.send_unchecked(request_to(&broker, Method::DELETE, "snapshots")?).await;
    expect_that!(&answer_7, answered(StatusCode::BAD_REQUEST));
    expect_that!(instance_of(&answer_7), some(eq("/ngsi-ld/v1/snapshots")));
    Ok(())
}
