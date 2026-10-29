//! The partial success of a typed call, required: a test that expects a 207 Multi-Status (clause 6.3.3) reads the
//! broker's report of what succeeded and what failed, and fails when the broker answered the whole success instead.

use bytes::Bytes;
use http::{Response, StatusCode};
use ngsi_ld::{
    client::ClientError,
    http::{BatchCreateOutcome, BatchOperationResult, BatchOutcome, CreateOutcome, UpdateOutcome, UpdateResult, UpsertOutcome},
};
use serde::de::DeserializeOwned;
use std::fmt::Debug;

/// The broker answered the whole success, with the outcome answering it.
#[derive(Debug, thiserror::Error)]
#[error("the broker answered the whole success, not a partial one: {0:?}")]
pub struct Whole<O: Debug>(O);

/// An outcome that is the whole success or a partial one.
pub trait PartialSuccess: Debug + Sized {
    /// The report a partial success carries.
    type Report;

    /// The report of the partial success.
    ///
    /// # Errors
    ///
    /// Returns [`Whole`] when the success is whole.
    fn partial(self) -> Result<Self::Report, Whole<Self>>;
}

impl PartialSuccess for CreateOutcome {
    type Report = BatchOperationResult;

    fn partial(self) -> Result<BatchOperationResult, Whole<Self>> {
        match self {
            CreateOutcome::PartiallyCreated { result, .. } => Ok(result),
            whole @ CreateOutcome::Created { .. } => Err(Whole(whole)),
        }
    }
}

impl PartialSuccess for BatchOutcome {
    type Report = BatchOperationResult;

    fn partial(self) -> Result<BatchOperationResult, Whole<Self>> {
        match self {
            BatchOutcome::Partial(result) => Ok(result),
            whole @ BatchOutcome::Done => Err(Whole(whole)),
        }
    }
}

impl PartialSuccess for BatchCreateOutcome {
    type Report = BatchOperationResult;

    fn partial(self) -> Result<BatchOperationResult, Whole<Self>> {
        match self {
            BatchCreateOutcome::Partial(result) => Ok(result),
            whole @ BatchCreateOutcome::Created(_) => Err(Whole(whole)),
        }
    }
}

impl PartialSuccess for UpsertOutcome {
    type Report = BatchOperationResult;

    fn partial(self) -> Result<BatchOperationResult, Whole<Self>> {
        match self {
            UpsertOutcome::Partial(result) => Ok(result),
            whole @ (UpsertOutcome::Created(_) | UpsertOutcome::Updated) => Err(Whole(whole)),
        }
    }
}

impl PartialSuccess for UpdateOutcome {
    type Report = UpdateResult;

    fn partial(self) -> Result<UpdateResult, Whole<Self>> {
        match self {
            UpdateOutcome::Partial(result) => Ok(result),
            whole @ UpdateOutcome::Done => Err(Whole(whole)),
        }
    }
}

/// Why the answer to an unchecked request holds no report of a partial success.
#[derive(Debug, thiserror::Error)]
pub enum UnreportedError<E: Debug> {
    /// The broker answered a success other than 207 Multi-Status.
    #[error("the broker answered {0}, not 207 Multi-Status")]
    Status(StatusCode),
    /// The broker refused the request or the transport failed.
    #[error("the request failed: {0:?}")]
    Failed(Box<ClientError<E>>),
    /// The body is not the report the operation answers.
    #[error("the 207 body is not a report: {0}")]
    Body(#[from] serde_json::Error),
}

/// The report of the partial success an unchecked request was answered with: a `BatchOperationResult` or an
/// `UpdateResult` (clause 6.3.3), read from the body of the 207, which `send_unchecked` returns as received.
///
/// # Errors
///
/// Returns [`UnreportedError`] when the answer is not a 207 or its body is not a report of type `R`.
pub fn reported<R: DeserializeOwned, E: Debug>(answer: Result<Response<Bytes>, ClientError<E>>) -> Result<R, UnreportedError<E>> {
    let response = answer.map_err(|error| UnreportedError::Failed(Box::new(error)))?;
    if response.status() != StatusCode::MULTI_STATUS {
        return Err(UnreportedError::Status(response.status()));
    }
    Ok(serde_json::from_slice(response.body())?)
}

#[cfg(test)]
mod tests {
    use crate::partial_outcome::{PartialSuccess, UnreportedError, reported};
    use bytes::Bytes;
    use googletest::prelude::*;
    use http::{Response, StatusCode};
    use ngsi_ld::{
        client::ClientError,
        http::{BatchOperationResult, BatchOutcome, UpdateOutcome, UpdateResult},
    };
    use std::{io, result::Result as StdResult};

    /// An answer to an unchecked request with `status` and `body`.
    fn answer(status: StatusCode, body: &'static str) -> StdResult<Response<Bytes>, ClientError<io::Error>> {
        Response::builder()
            .status(status)
            .body(Bytes::from_static(body.as_bytes()))
            .map_err(|error| ClientError::Transport(io::Error::other(error)))
    }

    #[gtest]
    fn a_multi_status_body_is_read_as_the_report() -> Result<()> {
        let report: BatchOperationResult = reported(answer(StatusCode::MULTI_STATUS, r#"{"success":["urn:ngsi-ld:Vehicle:A"],"errors":[]}"#))?;
        verify_that!(report.success, len(eq(1)))
    }

    #[gtest]
    fn another_success_status_is_no_report() -> Result<()> {
        let report: StdResult<UpdateResult, UnreportedError<io::Error>> = reported(answer(StatusCode::NO_CONTENT, ""));
        verify_that!(report, err(displays_as(contains_substring("204"))))
    }

    #[gtest]
    fn a_partial_success_yields_its_report_and_a_whole_one_fails() -> Result<()> {
        let report = BatchOperationResult::default();
        expect_that!(BatchOutcome::Partial(report.clone()).partial(), ok(eq(&report)));
        expect_that!(UpdateOutcome::Partial(UpdateResult::default()).partial(), ok(anything()));
        verify_that!(BatchOutcome::Done.partial(), err(anything()))
    }
}
