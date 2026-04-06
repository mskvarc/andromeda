use crate::{api::extractor::error::ExtractorRejection, http::header::NgsiLdSnapshot};
use axum::{extract::FromRequestParts, http::request::Parts};
use headers::HeaderMapExt;

/// Extractor for the `NGSILD-Snapshot` request header.
///
/// When present, the request is scoped to the frozen data of the specified
/// snapshot. The value must be a non-empty snapshot identifier.
#[derive(Clone, Debug, Default)]
pub struct SnapshotHeader(pub Option<String>);

impl SnapshotHeader {
    /// Consume the extractor and return the optional snapshot identifier.
    pub fn into_inner(self) -> Option<String> {
        self.0
    }
}

impl<S> FromRequestParts<S> for SnapshotHeader
where
    S: Send + Sync,
{
    type Rejection = ExtractorRejection;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let uri = parts.uri.clone();

        match parts.headers.typed_try_get::<NgsiLdSnapshot>() {
            Ok(Some(snapshot)) => Ok(Self(Some(snapshot.into_inner()))),
            Ok(None) => Ok(Self(None)),
            Err(_) => Err(ExtractorRejection::bad_request_data("NGSILD-Snapshot header must not be empty", uri)),
        }
    }
}
