//! The exact wire form of requests and answers, for a test whose subject is the representation itself: a request
//! target written exactly as given, and the header fields of an answer.

use bytes::Bytes;
use http::{
    HeaderMap,
    HeaderName,
    HeaderValue,
    Method,
    Response,
    Uri,
    header::{ACCEPT, CONTENT_TYPE, InvalidHeaderValue, VARY},
    uri::InvalidUri,
};
use mediatype::MediaType;
use ngsi_ld::{
    client::{Answering, Client, ClientError, Received, Transport},
    http::{
        RequestError,
        UncheckedRequest,
        operation::{Operation, RetrieveEntityTypes},
    },
};
use serde_json::Value;
use url::{ParseError, Url};

/// Andromeda's own API root (`/ngsi-ld/andromeda/v1/`), relative to the NGSI-LD API root (`/ngsi-ld/v1/`) a client
/// speaks to: a sibling namespace under the same `/ngsi-ld/` prefix, whatever path prefix the apiRoot carries.
const ANDROMEDA_ROOT_FROM_API_ROOT: &str = "../andromeda/v1/";

/// Why a wire value could not be built.
#[derive(Debug, thiserror::Error)]
pub enum WireError {
    /// A path on the API root is not a URI.
    #[error("a path on {0} is not a request target")]
    Target(Url, #[source] InvalidUri),
    /// Andromeda's own API root cannot be resolved against the client's API root.
    #[error("andromeda's own api root cannot be resolved against {0}")]
    AndromedaRoot(Url, #[source] ParseError),
    /// The client refused to build the request it starts from.
    #[error("the request could not be built")]
    Request(#[source] RequestError),
    /// A media type cannot be written as a header field.
    #[error("a media type cannot be written as a header field")]
    MediaType(#[source] InvalidHeaderValue),
}

/// The URI of `path` (a path with its query, relative to the API root) on the broker `client` speaks to, written
/// exactly as given: a trailing `/` or an encoding the typed operations would not produce is kept.
///
/// # Errors
///
/// Returns [`WireError::Target`] when the joined text is not a URI.
pub fn api_uri<T: Transport, M: Answering>(client: &Client<T, M>, path: &str) -> Result<Uri, WireError> {
    let base = client.endpoint().base_url();
    // clone: the error must own the API root it reports
    format!("{base}{path}").parse().map_err(|error| WireError::Target(base.clone(), error))
}

/// The URI of `path` (a path with its query, relative to Andromeda's own API root `/ngsi-ld/andromeda/v1/`) on the
/// broker `client` speaks to, written exactly as given.
///
/// # Errors
///
/// Returns [`WireError::AndromedaRoot`] when the root cannot be resolved and [`WireError::Target`] when the joined text
/// is not a URI.
pub fn andromeda_uri<T: Transport, M: Answering>(client: &Client<T, M>, path: &str) -> Result<Uri, WireError> {
    let base = client.endpoint().base_url();
    // clone: the error must own the API root it reports
    let root = base
        .join(ANDROMEDA_ROOT_FROM_API_ROOT)
        .map_err(|error| WireError::AndromedaRoot(base.clone(), error))?;
    format!("{root}{path}").parse().map_err(|error| WireError::Target(root, error))
}

/// A request with `method` to `path` (relative to the API root, with its query) on the broker `client` speaks to,
/// carrying the client's tenant and no body, `Accept` or `Content-Type`: the start of a request whose target no
/// operation writes, such as one with a parameter the specification does not define.
///
/// # Errors
///
/// Returns [`WireError`] when the target is not a URI.
pub fn request_to<T: Transport, M: Answering>(client: &Client<T, M>, method: Method, path: &str) -> Result<UncheckedRequest, WireError> {
    Ok(bare_request(client, method)?.uri(api_uri(client, path)?))
}

/// A request with `method` to `path` (relative to Andromeda's own API root `/ngsi-ld/andromeda/v1/`, with its query)
/// on the broker `client` speaks to, carrying the client's tenant and no body, `Accept` or `Content-Type`: the start
/// of a request to a route no NGSI-LD clause defines, which no typed operation writes.
///
/// # Errors
///
/// Returns [`WireError`] when the target is not a URI.
pub fn andromeda_request_to<T: Transport, M: Answering>(client: &Client<T, M>, method: Method, path: &str) -> Result<UncheckedRequest, WireError> {
    Ok(bare_request(client, method)?.uri(andromeda_uri(client, path)?))
}

/// A request with `method` to the API root of the broker `client` speaks to, carrying the client's tenant and no
/// body, `Accept` or `Content-Type`, for the caller to point at its target.
fn bare_request<T: Transport, M: Answering>(client: &Client<T, M>, method: Method) -> Result<UncheckedRequest, WireError> {
    let start = UncheckedRequest::try_from_operation(&RetrieveEntityTypes::builder().build(), client.endpoint()).map_err(WireError::Request)?;
    Ok(start.method(method).remove_header(&ACCEPT).remove_header(&CONTENT_TYPE))
}

/// Sends `operation` to `path` (relative to the API root) instead of the target the operation writes, for a target
/// such as one with a trailing `/` (`POST /entities/`), and answers with the response as received.
///
/// # Errors
///
/// Returns [`ClientError`] when the request cannot be built, the transport fails or the broker answers with an
/// error.
pub async fn send_to<T: Transport, M: Answering, O: Operation + Sync>(
    client: &Client<T, M>,
    operation: &O,
    path: &str,
) -> Result<Response<Bytes>, ClientError<T::Error>> {
    let uri = format!("{}{path}", client.endpoint().base_url())
        .parse()
        .map_err(|source| ClientError::Request(RequestError::Uri { source }))?;
    client.send_unchecked(client.unchecked(operation)?.uri(uri)).await
}

/// `request` with its `Content-Type` replaced by `media_type`, written as given: a test of a media type the broker
/// must refuse names one the typed operations never send.
///
/// # Errors
///
/// Returns [`InvalidHeaderValue`] when `media_type` holds a parameter value no header field can carry.
pub fn content_type(request: UncheckedRequest, media_type: &MediaType<'_>) -> Result<UncheckedRequest, InvalidHeaderValue> {
    Ok(request
        .remove_header(&CONTENT_TYPE)
        .append_header(CONTENT_TYPE, HeaderValue::try_from(media_type.to_string())?))
}

/// `request` with its `Accept` replaced by `media_range`, written as given: a test of content negotiation names a
/// media range, a list with weights or a media type the typed operations never accept.
#[must_use]
pub fn accept(request: UncheckedRequest, media_range: &'static str) -> UncheckedRequest {
    request.remove_header(&ACCEPT).append_header(ACCEPT, HeaderValue::from_static(media_range))
}

/// The body of `received` as a JSON tree, for an exact comparison or a read of one member.
///
/// # Errors
///
/// Returns the parse error when the body is not JSON.
pub fn json_body<V>(received: &Received<V>) -> Result<Value, serde_json::Error> {
    serde_json::from_slice(&received.body)
}

/// Every value of the header `name`, in the order the field lines came, as text; a value that is not visible ASCII
/// is left out.
#[must_use]
pub fn header_values<'a>(headers: &'a HeaderMap, name: &HeaderName) -> Vec<&'a str> {
    headers.get_all(name).iter().filter_map(|value| value.to_str().ok()).collect()
}

/// The single value of the header `name`, when exactly one field line carries it.
#[must_use]
pub fn header_value<'a>(headers: &'a HeaderMap, name: &HeaderName) -> Option<&'a str> {
    match header_values(headers, name).as_slice() {
        [value] => Some(value),
        [] | [_, _, ..] => None,
    }
}

/// The field names a `Vary` header lists (RFC 9110 section 12.5.5) over all its field lines; `*`, which names no field,
/// and a member that is not a field name are left out.
#[must_use]
pub fn vary(headers: &HeaderMap) -> Vec<HeaderName> {
    header_values(headers, &VARY)
        .into_iter()
        .flat_map(|line| line.split(',').map(str::trim))
        .filter(|member| *member != "*")
        .filter_map(|member| HeaderName::from_bytes(member.as_bytes()).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::{
        broker::client_at,
        wire::{accept, andromeda_uri, header_value, header_values, vary},
    };
    use googletest::prelude::*;
    use http::{
        HeaderMap,
        HeaderValue,
        header::{ACCEPT, ACCEPT_LANGUAGE, LINK, VARY},
    };
    use ngsi_ld::http::operation::RetrieveEntityTypes;
    use url::Url;

    #[gtest]
    fn accept_replaces_every_value_the_operation_wrote() -> Result<()> {
        let client = client_at(Url::parse("http://broker.test/ngsi-ld/v1/")?)?;
        let request = accept(client.unchecked(&RetrieveEntityTypes::builder().build())?, "text/*");
        verify_that!(header_values(request.as_request().headers(), &ACCEPT), elements_are![eq(&"text/*")])
    }

    #[gtest]
    fn andromeda_paths_sit_beside_the_api_root_under_any_prefix() -> Result<()> {
        for (api_root, expected) in [
            ("http://broker.test/ngsi-ld/v1/", "http://broker.test/ngsi-ld/andromeda/v1/tenants?limit=1"),
            (
                "http://broker.test/broker/ngsi-ld/v1/",
                "http://broker.test/broker/ngsi-ld/andromeda/v1/tenants?limit=1",
            ),
        ] {
            let client = client_at(Url::parse(api_root)?)?;
            expect_that!(andromeda_uri(&client, "tenants?limit=1")?.to_string(), eq(expected));
        }
        Ok(())
    }

    #[gtest]
    fn vary_lists_the_field_names_of_every_line() {
        let mut headers = HeaderMap::new();
        headers.append(VARY, HeaderValue::from_static("Accept, accept-language"));
        headers.append(VARY, HeaderValue::from_static("*"));
        expect_that!(vary(&headers), elements_are![eq(&ACCEPT), eq(&ACCEPT_LANGUAGE)]);
    }

    #[gtest]
    fn a_header_on_several_lines_has_no_single_value() {
        let mut headers = HeaderMap::new();
        headers.append(LINK, HeaderValue::from_static("<a>"));
        expect_that!(header_value(&headers, &LINK), some(eq("<a>")));
        headers.append(LINK, HeaderValue::from_static("<b>"));
        expect_that!(header_value(&headers, &LINK), none());
        expect_that!(header_values(&headers, &LINK), elements_are![eq(&"<a>"), eq(&"<b>")]);
    }
}
