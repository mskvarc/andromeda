//! Andromeda's temporal delete by query (`DELETE /temporal/entities`, clause 6.18.3 binds no `DELETE` to the
//! collection): the request, which no operation of the specification writes, and the count a dry run answers.

use crate::{
    broker::BrokerClient,
    wire::{WireError, request_to},
};
use bytes::Bytes;
use http::{Method, Response};
use ngsi_ld::http::UncheckedRequest;
use serde::Deserialize;

/// Why a dry run's answer is not a count of deleted instances.
#[derive(Debug, thiserror::Error)]
pub enum DryRunError {
    /// The body is not the `{"deleted": n}` object.
    #[error("the body is not a count of deleted instances")]
    NotACount(#[source] serde_json::Error),
}

/// What a dry run answers: the number of instances the window matches.
#[derive(Debug, Clone, Copy, Deserialize)]
struct DryRun {
    /// The instances the delete would remove.
    deleted: u64,
}

/// The request deleting the temporal evolutions the query string `query` selects, written as given.
///
/// # Errors
///
/// Returns [`WireError`] when the target is not a URI.
pub fn delete_request(client: &BrokerClient, query: &str) -> Result<UncheckedRequest, WireError> {
    request_to(client, Method::DELETE, &format!("temporal/entities?{query}"))
}

/// The number of instances a dry run answered it would delete.
///
/// # Errors
///
/// Returns [`DryRunError`] when the body is not a count.
pub fn dry_run_count(response: &Response<Bytes>) -> Result<u64, DryRunError> {
    serde_json::from_slice::<DryRun>(response.body())
        .map(|report| report.deleted)
        .map_err(DryRunError::NotACount)
}

#[cfg(test)]
mod tests {
    use crate::temporal_delete_by_query::dry_run_count;
    use bytes::Bytes;
    use googletest::prelude::*;
    use http::Response;

    #[gtest]
    fn the_count_of_a_dry_run_is_read() -> Result<()> {
        verify_that!(dry_run_count(&Response::new(Bytes::from_static(br#"{"deleted":2}"#)))?, eq(2))
    }

    #[gtest]
    fn a_body_without_a_count_is_refused() {
        expect_that!(dry_run_count(&Response::new(Bytes::from_static(b"{}"))).is_err(), eq(true));
    }
}
