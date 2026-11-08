//! The request a test sends to the `Location` a creating operation answered with, the relative path of the created
//! resource (Tables 6.4.3.1-1, 6.8.3.1-1, 6.10.3.1-1, 6.12.3.1-1, 6.18.3.1-1 and 6.36.3.1-1): the path is resolved
//! against the broker's address as a client resolves it (IETF RFC 3986 section 5.2), and the request carries the
//! client's tenant. The identifier of the resource the answer represents is read back from its `id` member.

use crate::{
    broker::BrokerClient,
    wire::{WireError, header_value, request_to},
};
use bytes::Bytes;
use http::{
    HeaderMap,
    HeaderValue,
    Method,
    Response,
    header::{ACCEPT, LOCATION},
    uri::InvalidUri,
};
use ngsi_ld::{http::UncheckedRequest, model::Uri};
use serde::Deserialize;

/// Why a `Location` could not be followed, or the answer names no resource.
#[derive(Debug, thiserror::Error)]
pub enum LocationError {
    /// The answer carries no `Location` that reads as text.
    #[error("the answer carries no readable Location")]
    NoLocation,
    /// The `Location` does not resolve against the broker's address.
    #[error("the Location {0} does not resolve")]
    Target(String, #[source] url::ParseError),
    /// The resolved `Location` is not a request target.
    #[error("the resolved Location is not a URI")]
    Uri(#[source] InvalidUri),
    /// The request could not be started.
    #[error(transparent)]
    Wire(#[from] WireError),
    /// The body is no resource representation with an `id`.
    #[error("the body names no resource identifier")]
    Body(#[source] serde_json::Error),
}

/// The member every resource representation carries (clause 5.2): its identifier.
#[derive(Debug, Deserialize)]
struct Identified {
    /// The identifier of the represented resource.
    id: Uri,
}

/// A GET request with `Accept: application/json` to the `Location` in `headers`, resolved against the API root of
/// `client`, with the path written exactly as the broker wrote it.
///
/// # Errors
///
/// Returns [`LocationError`] when `headers` carry no `Location` or it does not resolve.
pub fn request(client: &BrokerClient, headers: &HeaderMap) -> Result<UncheckedRequest, LocationError> {
    let location = header_value(headers, &LOCATION).ok_or(LocationError::NoLocation)?;
    let resolved = client
        .endpoint()
        .base_url()
        .join(location)
        .map_err(|error| LocationError::Target(location.to_owned(), error))?;
    let uri: http::Uri = resolved.as_str().parse().map_err(LocationError::Uri)?;
    Ok(request_to(client, Method::GET, "")?
        .uri(uri)
        .append_header(ACCEPT, HeaderValue::from_static("application/json")))
}

/// The identifier of the resource `response` represents.
///
/// # Errors
///
/// Returns [`LocationError::Body`] when the body is not a JSON object with a URI `id`.
pub fn identifier(response: &Response<Bytes>) -> Result<Uri, LocationError> {
    serde_json::from_slice::<Identified>(response.body())
        .map(|identified| identified.id)
        .map_err(LocationError::Body)
}

#[cfg(test)]
mod tests {
    use crate::{
        broker,
        followed_location::{LocationError, identifier, request},
    };
    use bytes::Bytes;
    use googletest::prelude::*;
    use http::{HeaderMap, HeaderValue, Response, header::LOCATION};
    use ngsi_ld::model::Uri;

    #[gtest]
    fn the_location_resolves_against_the_broker_with_its_escapes_kept() -> Result<()> {
        let client = broker::client_at("http://broker.test:1026/ngsi-ld/v1/".parse()?)?;
        let mut headers = HeaderMap::new();
        headers.insert(LOCATION, HeaderValue::from_static("/ngsi-ld/v1/entities/urn:ngsi-ld:Building:x%2Fy%2541%C4%8D"));
        let followed = request(&client, &headers)?;
        verify_that!(
            followed.as_request().uri().to_string(),
            eq("http://broker.test:1026/ngsi-ld/v1/entities/urn:ngsi-ld:Building:x%2Fy%2541%C4%8D")
        )
    }

    #[gtest]
    fn an_answer_without_a_location_cannot_be_followed() -> Result<()> {
        let client = broker::client_at("http://broker.test:1026/ngsi-ld/v1/".parse()?)?;
        verify_that!(matches!(request(&client, &HeaderMap::new()), Err(LocationError::NoLocation)), eq(true))
    }

    #[gtest]
    fn the_identifier_is_read_from_the_id_member() -> Result<()> {
        let response = Response::builder()
            .status(200)
            .body(Bytes::from_static(br#"{"id":"urn:ngsi-ld:Building:x/y","type":"Building"}"#))?;
        verify_that!(identifier(&response)?, eq(&"urn:ngsi-ld:Building:x/y".parse::<Uri>()?))
    }
}
