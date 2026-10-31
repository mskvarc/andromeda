//! The batch entity operations of the conformance tests and the checks on their full and partial results.

use andromeda_test_suite::{broker::BrokerClient, problem::answered};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    client::{ClientError, Received, ReqwestError},
    http::{BatchCreateOutcome, BatchOperationResult, BatchOutcome, UncheckedRequest, UpsertOutcome},
    model::{Entity, EntityId, ErrorType, Input, ProblemDetails},
};
use std::fmt::Debug;

/// `Batch Entity Create` of `entities` with `client`, requiring 201: answers the ids the broker reports as created.
pub(crate) async fn batch_create(client: &BrokerClient, entities: &[Entity<Input>]) -> Result<Vec<EntityId>> {
    let created = match client.entities().batch_create(entities).send().await? {
        BatchCreateOutcome::Created(ids) => Some(ids),
        BatchCreateOutcome::Partial(_) => None,
    };
    created.or_fail()
}

/// `Batch Entity Create` of `entities` with `client`, requiring 207: answers the partial result the broker reports.
pub(crate) async fn batch_create_partial(client: &BrokerClient, entities: &[Entity<Input>]) -> Result<BatchOperationResult> {
    let partial = match client.entities().batch_create(entities).send().await? {
        BatchCreateOutcome::Partial(result) => Some(result),
        BatchCreateOutcome::Created(_) => None,
    };
    partial.or_fail()
}

/// Sends `request` with `client`, requires 207, and answers the partial result the broker reports.
pub(crate) async fn partial_batch_answer(client: &BrokerClient, request: UncheckedRequest) -> Result<BatchOperationResult> {
    let answer = client.send_unchecked(request).await;
    verify_that!(&answer, answered(StatusCode::MULTI_STATUS))?;
    Ok(serde_json::from_slice(answer?.body())?)
}

/// Requires the partial result of a batch in which exactly the entities `succeeded` were treated and `failed` is the
/// one entity in error, with a non-empty problem.
pub(crate) fn expect_partial_batch(result: &BatchOperationResult, succeeded: &[&EntityId], failed: &EntityId) -> Result<()> {
    expect_that!(result.success.len(), eq(succeeded.len()));
    for id in succeeded {
        expect_that!(&result.success, contains(eq(*id)));
    }
    expect_that!(result.errors.len(), eq(1));
    let error = result.errors.first().or_fail()?;
    expect_that!(&error.entity_id, eq(failed));
    expect_that!(&error.error, not(eq(&ProblemDetails::default())));
    Ok(())
}

/// Requires the answer of a typed operation that succeeds without content (a batch delete, merge, update or upsert
/// that reports every entity done, or a subscription delete) to be the 204 with an empty body.
pub(crate) fn expect_batch_done<O: Debug>(answer: Result<Received<O>, ClientError<ReqwestError>>) -> Result<()> {
    verify_that!(&answer, answered(StatusCode::NO_CONTENT))?;
    expect_that!(answer?.body.len(), eq(0));
    Ok(())
}

/// Requires the answer of a batch upsert to be the 201 that lists exactly the entities `created` as created, and no
/// other entity.
pub(crate) fn expect_upsert_created(answer: Result<Received<UpsertOutcome>, ClientError<ReqwestError>>, created: &[&EntityId]) -> Result<()> {
    verify_that!(&answer, answered(StatusCode::CREATED))?;
    let ids = match answer?.outcome {
        UpsertOutcome::Created(ids) => Some(ids),
        UpsertOutcome::Updated | UpsertOutcome::Partial(_) => None,
    };
    let ids = ids.or_fail()?;
    expect_that!(ids.len(), eq(created.len()));
    for id in created {
        expect_that!(&ids, contains(eq(*id)));
    }
    Ok(())
}

/// Requires the answer of a typed batch operation (delete, merge or update) to be the 207 that reports entities in
/// error, and answers the partial result it carries.
pub(crate) fn partial_batch_outcome(answer: Result<Received<BatchOutcome>, ClientError<ReqwestError>>) -> Result<BatchOperationResult> {
    verify_that!(&answer, answered(StatusCode::MULTI_STATUS))?;
    let partial = match answer?.outcome {
        BatchOutcome::Partial(result) => Some(result),
        BatchOutcome::Done => None,
    };
    partial.or_fail()
}

/// Requires the first error of a partial batch result to be `BadRequestData` with a non-empty title: the refusal of an
/// entity whose body is not a valid entity.
pub(crate) fn expect_bad_request_entity(result: &BatchOperationResult) -> Result<()> {
    let error = result.errors.first().or_fail()?;
    expect_that!(&error.error.error_type, eq(&Some(ErrorType::BadRequestData)));
    expect_that!(error.error.title.as_deref(), some(not(eq(""))));
    Ok(())
}
