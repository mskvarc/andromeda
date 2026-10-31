//! A request written down to the byte, sent without the client's reading of the answer, for a test whose subject is
//! an exchange no typed operation states: a query string the parameter models refuse, a body the model has no place
//! for, a method an endpoint does not define. The answer comes back whatever its status, with its header fields and
//! body as received, to be read with the matchers of [`crate::body_json`] and [`crate::header_matcher`].

use crate::{
    broker::BrokerClient,
    raw_answer::{RawAnswerError, send_raw},
    request_fields::{FieldError, with_fields},
    wire::{WireError, request_to},
};
use bytes::Bytes;
use http::{Method, Response, header::CONTENT_TYPE};
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};

/// The characters of a query name or value that go out as they are: the unreserved set of IETF RFC 3986 section 2.3.
const UNRESERVED: &AsciiSet = &NON_ALPHANUMERIC.remove(b'-').remove(b'.').remove(b'_').remove(b'~');

/// Why a raw call could not be made.
#[derive(Debug, thiserror::Error)]
pub enum RawCallError {
    /// The target is not a request target.
    #[error(transparent)]
    Target(#[from] WireError),
    /// A header field cannot be written.
    #[error(transparent)]
    Field(#[from] FieldError),
    /// The request could not be sent or the answer not received.
    #[error(transparent)]
    Answer(#[from] RawAnswerError),
}

/// `path` (relative to the API root) followed by `parameters` as a query string, each name and value
/// percent-encoded, in order; `path` alone when there are none.
#[must_use]
pub fn target(path: &str, parameters: &[(&str, &str)]) -> String {
    if parameters.is_empty() {
        return path.to_owned();
    }
    let query = parameters
        .iter()
        .map(|(name, value)| format!("{}={}", utf8_percent_encode(name, UNRESERVED), utf8_percent_encode(value, UNRESERVED)))
        .collect::<Vec<_>>()
        .join("&");
    format!("{path}?{query}")
}

/// Sends `method` to `target` (relative to the API root, with its query, as written) on the broker `client` speaks to,
/// with `headers` in order and `body` byte for byte. A body goes out under `application/json` unless `headers` name
/// a `Content-Type`.
///
/// # Errors
///
/// Returns [`RawCallError`] when the request cannot be written, sent or answered.
pub async fn call(client: &BrokerClient, method: Method, target: &str, headers: &[(&str, &str)], body: Option<&str>) -> Result<Response<Bytes>, RawCallError> {
    let request = with_fields(request_to(client, method, target)?, headers)?;
    let names_content_type = headers.iter().any(|(name, _)| name.eq_ignore_ascii_case(CONTENT_TYPE.as_str()));
    let request = match body {
        Some(text) if names_content_type => request.body(Bytes::copy_from_slice(text.as_bytes())),
        Some(text) => with_fields(request, &[("Content-Type", "application/json")])?.body(Bytes::copy_from_slice(text.as_bytes())),
        None => request,
    };
    Ok(send_raw(request).await?)
}

#[cfg(test)]
mod tests {
    use crate::{
        broker::client_at,
        raw_call::{call, target},
    };
    use bytes::Bytes;
    use googletest::prelude::*;
    use http::{Method, StatusCode};
    use url::Url;
    use wiremock::{
        Mock,
        MockServer,
        ResponseTemplate,
        matchers::{body_string, header, method, query_param},
    };

    #[gtest]
    fn parameters_are_encoded_in_order_and_none_leaves_the_path() {
        expect_that!(target("snapshots", &[]), eq("snapshots"));
        expect_that!(
            target("snapshots", &[("q", r#"a=="b c";d>1"#), ("limit", "2")]),
            eq("snapshots?q=a%3D%3D%22b%20c%22%3Bd%3E1&limit=2")
        );
    }

    #[gtest]
    #[tokio::test]
    async fn a_body_goes_out_under_json_and_the_answer_comes_back_whatever_its_status() -> Result<()> {
        let server = MockServer::start().await;
        Mock::given(method("PATCH"))
            .and(query_param("q", "a;b"))
            .and(header("content-type", "application/json"))
            .and(body_string(r#"{ "x":1 }"#))
            .respond_with(ResponseTemplate::new(400).set_body_string("no"))
            .mount(&server)
            .await;
        let client = client_at(Url::parse(&format!("{}/ngsi-ld/v1/", server.uri()))?)?;
        let answer = call(&client, Method::PATCH, &target("things", &[("q", "a;b")]), &[], Some(r#"{ "x":1 }"#)).await?;
        expect_that!(answer.status(), eq(StatusCode::BAD_REQUEST));
        expect_that!(answer.body(), eq(&Bytes::from_static(b"no")));
        Ok(())
    }
}
