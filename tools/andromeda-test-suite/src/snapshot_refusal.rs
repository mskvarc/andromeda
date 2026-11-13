//! The answer to a request naming a Snapshot on a route outside the operations clause 5.5.15 permits on one: the
//! Registry, Snapshot, `@context` and Distributed support routes, and Andromeda's own.

use crate::problem::{AnsweredMatcher, problem};
use http::{HeaderName, StatusCode};
use ngsi_ld::{model::ErrorType, snapshot::SnapshotId};

/// The `detail` of every such refusal, naming the operations that take a Snapshot.
const DETAIL: &str = "ngsild-snapshot is supported only by the core, temporal, and entity-subscription operations";

/// The header naming the Snapshot, which clause 6.3.22 puts on the answer to a request carrying it.
const NGSILD_SNAPSHOT: HeaderName = HeaderName::from_static("ngsild-snapshot");

/// Matches the refusal of a request naming `snapshot`: 422 `OperationNotSupported` whose `detail` names the
/// operations that take a Snapshot, carrying `NGSILD-Snapshot` as the request did.
#[must_use]
pub fn refused_on_snapshot(snapshot: &SnapshotId) -> AnsweredMatcher {
    problem(StatusCode::UNPROCESSABLE_ENTITY, ErrorType::OperationNotSupported)
        .with_detail(DETAIL)
        .with_header(NGSILD_SNAPSHOT, &snapshot.to_string())
}

#[cfg(test)]
mod tests {
    use crate::snapshot_refusal::{DETAIL, NGSILD_SNAPSHOT, refused_on_snapshot};
    use bytes::Bytes;
    use googletest::prelude::*;
    use http::{HeaderMap, HeaderValue, Response, StatusCode};
    use ngsi_ld::{
        client::ClientError,
        http::{ResponseError, ResponseHead},
        model::{ErrorType, ProblemDetails, RawJson},
        snapshot::SnapshotId,
    };
    use std::{io, result::Result as StdResult};

    /// The answer of an unchecked request in these tests.
    type Answer = StdResult<Response<Bytes>, ClientError<io::Error>>;

    /// A 422 `OperationNotSupported` with `detail`, carrying `NGSILD-Snapshot: snapshot` when one is given.
    fn refusal(detail: &str, snapshot: Option<&'static str>) -> Result<Answer> {
        let mut headers = HeaderMap::new();
        if let Some(snapshot) = snapshot {
            headers.insert(NGSILD_SNAPSHOT, HeaderValue::from_static(snapshot));
        }
        let details = ProblemDetails::builder()
            .error_type(ErrorType::OperationNotSupported)
            .title("Operation not supported")
            .detail(detail)
            .build();
        Ok(Err(ClientError::Response(ResponseError::Problem {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            head: Box::new(ResponseHead::new(headers)),
            details: Box::new(details),
            body: Box::new("{}".parse::<RawJson>()?),
        })))
    }

    #[gtest]
    fn the_refusal_naming_the_scoped_operations_and_carrying_the_snapshot_matches() -> Result<()> {
        let snapshot = "urn:ngsi-ld:Snapshot:1".parse::<SnapshotId>()?;
        verify_that!(&refusal(DETAIL, Some("urn:ngsi-ld:Snapshot:1"))?, refused_on_snapshot(&snapshot))
    }

    #[gtest]
    fn a_refusal_without_the_snapshot_or_with_another_detail_does_not_match() -> Result<()> {
        let snapshot = "urn:ngsi-ld:Snapshot:1".parse::<SnapshotId>()?;
        expect_that!(&refusal(DETAIL, None)?, not(refused_on_snapshot(&snapshot)));
        expect_that!(&refusal(DETAIL, Some("urn:ngsi-ld:Snapshot:2"))?, not(refused_on_snapshot(&snapshot)));
        verify_that!(
            &refusal("the operation is not supported", Some("urn:ngsi-ld:Snapshot:1"))?,
            not(refused_on_snapshot(&snapshot))
        )
    }
}
