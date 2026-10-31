//! A `Prefer` header sent as the test writes it, and the `Preference-Applied` header of a raw answer.
//!
//! The typed `Prefer` header writes the `ngsi-ld` preference before `body` and refuses a `body` value outside `json`
//! and `ld+json`. A test of how the broker reads the preferences in the other order, or an unknown body value, sends
//! the text itself (IETF RFC 7240 section 2 lets a client list preferences in any order).

use crate::{
    broker::BrokerClient,
    unchecked_outcome::read_as,
    wire::{header_value, header_values},
};
use http::{HeaderMap, HeaderName, HeaderValue, StatusCode};
use ngsi_ld::{
    client::{ClientError, ReqwestError},
    http::{ResponseError, ResponseHead, UncheckedRequest, operation::Operation},
};

/// The `Prefer` header name.
static PREFER: HeaderName = HeaderName::from_static("prefer");

/// The `Preference-Applied` header name.
static PREFERENCE_APPLIED: HeaderName = HeaderName::from_static("preference-applied");

/// `request` with its `Prefer` header replaced by `text`, written as given.
#[must_use]
pub fn prefer_as_written(request: UncheckedRequest, text: &'static str) -> UncheckedRequest {
    request.remove_header(&PREFER).append_header(PREFER.clone(), HeaderValue::from_static(text))
}

/// Why an operation sent with a written `Prefer` header could not be answered.
#[derive(Debug, thiserror::Error)]
pub enum PreferredError {
    /// The request could not be built or the broker refused it.
    #[error("the request failed")]
    Client(#[source] Box<ClientError<ReqwestError>>),
    /// The success answer is not one the operation defines.
    #[error("the answer could not be read")]
    Read(#[source] Box<ResponseError>),
}

/// What a broker answered an operation sent with a written `Prefer` header: the status, the head and the outcome the
/// operation reads.
#[derive(Debug)]
pub struct Preferred<T> {
    /// The status of the answer.
    pub status: StatusCode,
    /// The head of the answer.
    pub head: ResponseHead,
    /// The outcome the operation reads from the answer.
    pub outcome: T,
}

/// Sends `operation` through `client` with its `Prefer` header written as `prefer`, and reads the answer through the
/// operation, so it is held to the same model as a typed call.
///
/// # Errors
///
/// Returns [`PreferredError`] when the request cannot be built, the broker refuses it or the answer is not one the
/// operation defines.
pub async fn send_preferring<O: Operation + Sync>(client: &BrokerClient, operation: &O, prefer: &'static str) -> Result<Preferred<O::Outcome>, PreferredError> {
    let request = prefer_as_written(client.unchecked(operation).map_err(|error| PreferredError::Client(Box::new(error)))?, prefer);
    // Sending takes the request, and the operation reads the answer against the request it answers.
    let sent = request.as_request().clone();
    let response = client.send_unchecked(request).await.map_err(|error| PreferredError::Client(Box::new(error)))?;
    let status = response.status();
    let head = ResponseHead::new(response.headers().clone());
    let outcome = read_as(operation, &sent, response).map_err(|error| PreferredError::Read(Box::new(error)))?;
    Ok(Preferred { status, head, outcome })
}

/// The one value of the `Preference-Applied` header of an answer, when exactly one field line carries it.
#[must_use]
pub fn preference_applied(headers: &HeaderMap) -> Option<&str> {
    header_value(headers, &PREFERENCE_APPLIED)
}

/// Whether any `Preference-Applied` field line of an answer names the `ngsi-ld` version preference.
#[must_use]
pub fn names_a_version(headers: &HeaderMap) -> bool {
    header_values(headers, &PREFERENCE_APPLIED).iter().any(|line| line.contains("ngsi-ld"))
}

#[cfg(test)]
mod tests {
    use crate::prefer_text::{names_a_version, prefer_as_written, preference_applied};
    use googletest::prelude::*;
    use http::{HeaderMap, HeaderValue};
    use ngsi_ld::http::{Endpoint, UncheckedRequest, operation::RetrieveEntityTypes};
    use url::Url;

    #[gtest]
    fn the_prefer_text_replaces_the_header_as_given() -> Result<()> {
        let endpoint = Endpoint::builder().base_url(Url::parse("http://broker.test/ngsi-ld/v1/")?).build()?;
        let request = UncheckedRequest::try_from_operation(&RetrieveEntityTypes::builder().build(), &endpoint)?;
        let request = prefer_as_written(prefer_as_written(request, "ngsi-ld=1.3"), "body=json, ngsi-ld=1.3");
        let prefer = request.as_request().headers().get_all("prefer").iter().collect::<Vec<_>>();
        verify_that!(prefer, elements_are![eq(&HeaderValue::from_static("body=json, ngsi-ld=1.3"))])
    }

    #[gtest]
    fn a_preference_applied_header_is_read_as_text() -> Result<()> {
        let mut headers = HeaderMap::new();
        expect_that!(preference_applied(&headers), none());
        expect_that!(names_a_version(&headers), eq(false));
        headers.insert("preference-applied", HeaderValue::from_static("ngsi-ld=1.0"));
        expect_that!(preference_applied(&headers), some(eq("ngsi-ld=1.0")));
        verify_that!(names_a_version(&headers), eq(true))
    }
}
