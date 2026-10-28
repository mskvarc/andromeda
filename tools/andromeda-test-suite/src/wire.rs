//! The exact wire form of requests and answers, for a test whose subject is the representation itself: a request
//! target written as the ETSI keyword writes it, and the header fields of an answer.

use bytes::Bytes;
use http::{
    HeaderMap,
    HeaderName,
    HeaderValue,
    Method,
    Response,
    Uri,
    header::{ACCEPT, CONTENT_TYPE, VARY},
    uri::InvalidUri,
};
use ngsi_ld::{
    client::{Answering, Client, ClientError, Received, Transport},
    http::{
        RequestError,
        UncheckedRequest,
        operation::{Operation, RetrieveEntityTypes},
    },
};
use serde_json::Value;

/// Why a wire value could not be built.
#[derive(Debug, thiserror::Error)]
pub enum WireError {
    /// The target is not a URI.
    #[error("{0} is not a request target")]
    Target(String, #[source] InvalidUri),
    /// The client refused to build the request it starts from.
    #[error("the request could not be built: {0}")]
    Request(String),
}

/// The URI of `path` (a path with its query, relative to the API root) on the broker `client` speaks to, written
/// exactly as given: a trailing `/` or an encoding the typed operations would not produce is kept.
///
/// # Errors
///
/// Returns [`WireError::Target`] when the joined text is not a URI.
pub fn api_uri<T: Transport, M: Answering>(client: &Client<T, M>, path: &str) -> Result<Uri, WireError> {
    let text = format!("{}{path}", client.endpoint().base_url());
    text.parse().map_err(|error| WireError::Target(text, error))
}

/// A request with `method` to `path` (relative to the API root, with its query) on the broker `client` speaks to,
/// carrying the client's tenant and no body, `Accept` or `Content-Type`: the start of a request whose target no
/// operation writes, such as one with a parameter the specification does not define.
///
/// # Errors
///
/// Returns [`WireError`] when the target is not a URI.
pub fn request_to<T: Transport, M: Answering>(client: &Client<T, M>, method: Method, path: &str) -> Result<UncheckedRequest, WireError> {
    let start = client
        .unchecked(&RetrieveEntityTypes::builder().build())
        .map_err(|error: ClientError<T::Error>| WireError::Request(error.to_string()))?;
    Ok(start
        .method(method)
        .uri(api_uri(client, path)?)
        .remove_header(&ACCEPT)
        .remove_header(&CONTENT_TYPE))
}

/// Sends `operation` to `path` (relative to the API root) instead of the target the operation writes, as an ETSI
/// keyword sends it with a trailing `/` (`POST /entities/`), and answers with the response as received.
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
#[must_use]
pub fn content_type(request: UncheckedRequest, media_type: &'static str) -> UncheckedRequest {
    request
        .remove_header(&CONTENT_TYPE)
        .append_header(CONTENT_TYPE, HeaderValue::from_static(media_type))
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
    use crate::wire::{header_value, header_values, vary};
    use googletest::prelude::*;
    use http::{
        HeaderMap,
        HeaderValue,
        header::{ACCEPT, ACCEPT_LANGUAGE, LINK, VARY},
    };

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
