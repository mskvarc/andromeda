//! What a broker records of a stored `@context`, read through Serve @context with `details=true` (clause 6.30.3.1).
//!
//! A downloaded `@context` is a `Cached` entry named by the whole URL it came from, percent-encoded into one path
//! segment (clause 5.13.2.4). The typed `serve_details` writes that segment its own way, so the record is read
//! through the request as written and [`context_details`] reads the answer.

use crate::{
    broker::BrokerClient,
    context_details,
    eventually::retried,
    problem::{answered, problem},
    wire::{WireError, request_to},
};
use bytes::Bytes;
use googletest::prelude::*;
use http::{Method, Response, StatusCode, Uri, uri::InvalidUri};
use ngsi_ld::{
    client::{ClientError, ReqwestError},
    model::{ContextMetadata, ErrorType, StoredContextKind},
};
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use url::Url;

/// The characters of an identifier that stay unescaped in a path segment.
const SEGMENT: &AsciiSet = &NON_ALPHANUMERIC.remove(b'-').remove(b'_').remove(b'.').remove(b'~');

/// Why a record could not be read.
#[derive(Debug, thiserror::Error)]
pub enum ContextRecordError {
    /// The request target is not a URI.
    #[error(transparent)]
    Wire(#[from] WireError),
    /// The location names no stored context.
    #[error("the location {0} names no stored context")]
    Location(Uri),
    /// The location names no stored context and is not a URI reference.
    #[error("the location names no stored context and is not a URI reference")]
    LocationNotUri(#[source] InvalidUri),
    /// The answer is not the metadata of a stored context.
    #[error(transparent)]
    Metadata(#[from] context_details::ContextDetailsError),
}

/// The path segment naming the context downloaded from `url`: the whole URL, percent-encoded.
#[must_use]
pub fn segment_of(url: &Url) -> String {
    utf8_percent_encode(url.as_str(), SEGMENT).to_string()
}

/// The path segment of the stored context a `location` names (clause 5.13.2.4): what follows `jsonldContexts/`.
///
/// # Errors
///
/// Returns [`ContextRecordError::Location`] when the location holds no segment after `jsonldContexts/`, or
/// [`ContextRecordError::LocationNotUri`] when it is not a URI reference either.
pub fn segment_of_location(location: &str) -> Result<&str, ContextRecordError> {
    location
        .rsplit_once("jsonldContexts/")
        .map(|(_, segment)| segment)
        .filter(|segment| !segment.is_empty() && !segment.contains(['?', '/']))
        .ok_or_else(|| {
            location
                .parse::<Uri>()
                .map_or_else(ContextRecordError::LocationNotUri, ContextRecordError::Location)
        })
}

/// Serve @context with `details=true` for the context named by `segment`, as answered.
async fn read(client: &BrokerClient, segment: &str) -> Result<Result<Response<Bytes>, ClientError<ReqwestError>>, ContextRecordError> {
    let request = request_to(client, Method::GET, &format!("jsonldContexts/{segment}?details=true"))?;
    Ok(client.send_unchecked(request).await)
}

/// The metadata `answer` carries.
fn metadata(answer: &Result<Response<Bytes>, ClientError<ReqwestError>>) -> Result<ContextMetadata> {
    let response = answer.as_ref().or_fail()?;
    Ok(context_details::metadata(response).map_err(ContextRecordError::Metadata)?)
}

/// Requires that the broker holds the context named by `segment`, and reads its record.
///
/// # Errors
///
/// Fails when the broker answers anything but 200 or a record that is not context metadata.
pub async fn stored(client: &BrokerClient, segment: &str) -> Result<ContextMetadata> {
    let answer = read(client, segment).await?;
    verify_that!(&answer, answered(StatusCode::OK))?;
    metadata(&answer)
}

/// Requires that the broker records the download of `url` as a `Cached` entry under that URL, waiting for the
/// record: the broker writes it after the request that made the download.
///
/// # Errors
///
/// Fails when no such record appears within the delivery deadline.
pub async fn cached_under(client: &BrokerClient, url: &Url) -> Result<()> {
    let segment = segment_of(url);
    retried(async || {
        let record = stored(client, &segment).await?;
        verify_that!(record.kind, eq(&StoredContextKind::Cached))?;
        verify_that!(&record.url, eq(url))
    })
    .await
}

/// Requires that the broker holds no context under the download URL `url`: Serve @context answers 404
/// `ResourceNotFound` (clause 5.13.4.4).
///
/// # Errors
///
/// Fails when the broker answers anything else.
pub async fn unrecorded(client: &BrokerClient, url: &Url) -> Result<()> {
    let answer = read(client, &segment_of(url)).await?;
    verify_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound))
}

#[cfg(test)]
mod tests {
    use crate::context_record::{segment_of, segment_of_location};
    use googletest::prelude::*;
    use url::Url;

    #[gtest]
    fn a_download_url_becomes_one_escaped_path_segment() -> Result<()> {
        let url = Url::parse("http://runner:9000/jc/ns/page.jsonld")?;
        verify_that!(segment_of(&url), eq("http%3A%2F%2Frunner%3A9000%2Fjc%2Fns%2Fpage.jsonld"))
    }

    #[gtest]
    fn a_location_names_the_segment_after_the_collection() -> Result<()> {
        verify_that!(
            segment_of_location("/ngsi-ld/v1/jsonldContexts/urn:ngsi-ld:JsonLdContext:a1")?,
            eq("urn:ngsi-ld:JsonLdContext:a1")
        )
    }

    #[gtest]
    fn a_location_with_no_segment_or_a_query_names_no_context() {
        expect_that!(segment_of_location("/ngsi-ld/v1/jsonldContexts/").is_err(), eq(true));
        expect_that!(segment_of_location("/ngsi-ld/v1/jsonldContexts/a?x=1").is_err(), eq(true));
        expect_that!(segment_of_location("/ngsi-ld/v1/entities/a").is_err(), eq(true));
    }
}
