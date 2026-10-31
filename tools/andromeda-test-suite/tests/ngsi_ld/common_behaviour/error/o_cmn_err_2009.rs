//! Entity batch operations with an unknown parameter are invalid request.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    problem::{answered, problem},
    wire::api_uri,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        headers::Accept,
        operation::{BatchCreate, BatchDelete, BatchMerge, BatchUpdate, BatchUpsert, QueryEntities, QueryMethod},
    },
    model::{Entity, EntityFragment, ErrorType, Lossless, NgsiLdMediaType},
    query::builders::SelectorTypeBuilder,
};

/// Pins: the entity batch operations (create, upsert, update, merge, delete) and Query Entities by POST with a URL
/// parameter they do not define answer `InvalidRequest` and change nothing
/// Clause: 6.3.20, 5.6.7, 5.6.8, 5.6.9, 5.6.10, 5.6.20, 5.7.2
/// Recorded: 6.3.20 says an incompatible parameter "should" give `InvalidRequest`
#[gtest]
#[tokio::test]
async fn o_cmn_err_2009_entity_batch_operations_with_an_unknown_parameter_are_invalid_request() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let device_id = namespace.urn("Device", 1)?;
    let device_2_id = namespace.urn("Device", 2)?;
    let device_3_id = namespace.urn("Device", 3)?;
    let device_4_id = namespace.urn("Device", 4)?;
    let device_5_id = namespace.urn("Device", 5)?;
    let device_type = namespace.entity_type("Device")?;
    let creations = [Entity::builder().id(device_id.clone()).entity_type(device_type.clone()).build()?];
    // The operation defines no `invalidParam` parameter; the request carries it as written.
    let request = broker
        .unchecked(&BatchCreate::builder().entities(&creations).build())?
        .uri(api_uri(&broker, "entityOperations/create?invalidParam=invalidValue")?);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    let upserts = [Entity::builder().id(device_2_id.clone()).entity_type(device_type.clone()).build()?];
    // The operation defines no `invalidParam` parameter; the request carries it as written.
    let request_2 = broker
        .unchecked(&BatchUpsert::builder().entities(&upserts).build())?
        .uri(api_uri(&broker, "entityOperations/upsert?invalidParam=invalidValue")?);
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    let updates = [EntityFragment::for_entity(device_3_id.clone(), device_type.clone()).build()?];
    // The operation defines no `invalidParam` parameter; the request carries it as written.
    let request_3 = broker
        .unchecked(&BatchUpdate::builder().fragments(&updates).build())?
        .uri(api_uri(&broker, "entityOperations/update?invalidParam=invalidValue")?);
    let answer_3 = broker.send_unchecked(request_3).await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    let merges = [EntityFragment::for_entity(device_4_id.clone(), device_type.clone()).build()?];
    // The operation defines no `invalidParam` parameter; the request carries it as written.
    let request_4 = broker
        .unchecked(&BatchMerge::builder().fragments(&merges).build())?
        .uri(api_uri(&broker, "entityOperations/merge?invalidParam=invalidValue")?);
    let answer_4 = broker.send_unchecked(request_4).await;
    expect_that!(&answer_4, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    let deletions = [device_5_id];
    // The operation defines no `invalidParam` parameter; the request carries it as written.
    let request_5 = broker
        .unchecked(&BatchDelete { ids: &deletions })?
        .uri(api_uri(&broker, "entityOperations/delete?invalidParam=invalidValue")?);
    let answer_5 = broker.send_unchecked(request_5).await;
    expect_that!(&answer_5, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    let selector = SelectorTypeBuilder::from(&device_type).build()?;
    // The operation defines no `invalidParam` parameter; the request carries it as written.
    let request_6 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .unchecked(
            &QueryEntities::builder()
                .entity_type(&selector)
                .method(QueryMethod::Post)
                .format(Lossless::Normalized)
                .build(),
        )?
        .uri(api_uri(&broker, "entityOperations/query?invalidParam=invalidValue")?);
    let answer_6 = broker.send_unchecked(request_6).await;
    expect_that!(&answer_6, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    let answer_7 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&device_id)
        .send()
        .await;
    expect_that!(&answer_7, answered(StatusCode::NOT_FOUND));
    let answer_8 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&device_2_id)
        .send()
        .await;
    expect_that!(&answer_8, answered(StatusCode::NOT_FOUND));
    Ok(())
}
