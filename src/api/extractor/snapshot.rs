use crate::api::extractor::error::ExtractorRejection;
use axum::{extract::FromRequestParts, http::request::Parts};

/// HTTP header name for NGSI-LD snapshot scoping (ETSI GS CIM 009 clause 6.3.22).
pub const NGSILD_SNAPSHOT_HEADER: &str = "NGSILD-Snapshot";

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
        let Some(raw) = parts.headers.get(NGSILD_SNAPSHOT_HEADER) else {
            return Ok(Self(None));
        };

        let value = raw
            .to_str()
            .map_err(|_| ExtractorRejection::bad_request_data("Invalid NGSILD-Snapshot header", uri.clone()))?
            .trim();

        if value.is_empty() {
            return Err(ExtractorRejection::bad_request_data("NGSILD-Snapshot header must not be empty", uri));
        }

        Ok(Self(Some(value.to_string())))
    }
}
