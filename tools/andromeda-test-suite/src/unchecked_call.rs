//! A request written down to the byte and sent through the typed client, so its answer is read with the matchers of
//! the typed answers ([`crate::problem`]): for a test whose subject is the broker's refusal of, or answer to, an input
//! the typed operations cannot state (a query string the parameter models refuse, a body with a member the model has no
//! place for, a media type the operation never sends).

use crate::{
    broker::BrokerClient,
    request_fields::{FieldError, with_fields},
    wire::{WireError, request_to},
};
use bytes::Bytes;
use http::Method;
use ngsi_ld::http::UncheckedRequest;

/// Why an unchecked call could not be written.
#[derive(Debug, thiserror::Error)]
pub enum UncheckedCallError {
    /// The target is not a request target.
    #[error(transparent)]
    Target(#[from] WireError),
    /// A header field cannot be written.
    #[error(transparent)]
    Field(#[from] FieldError),
}

/// The request with `method` to `target` (relative to the API root, with its query as written), carrying `fields` in
/// order and `body` byte for byte. No field is added beside those named: the typed client's `Content-Type` and
/// `Accept` are removed.
///
/// # Errors
///
/// Returns [`UncheckedCallError`] when the target is not a URI or a field cannot be written.
pub fn exchange(
    client: &BrokerClient,
    method: Method,
    target: &str,
    fields: &[(&str, &str)],
    body: Option<&str>,
) -> Result<UncheckedRequest, UncheckedCallError> {
    let request = with_fields(request_to(client, method, target)?, fields)?;
    Ok(match body {
        Some(text) => request.body(Bytes::copy_from_slice(text.as_bytes())),
        None => request,
    })
}

#[cfg(test)]
mod tests {
    use crate::{broker::client_at, unchecked_call::exchange};
    use bytes::Bytes;
    use googletest::prelude::*;
    use http::{
        Method,
        header::{ACCEPT, CONTENT_TYPE},
        uri::PathAndQuery,
    };
    use url::Url;

    #[gtest]
    fn the_body_and_the_named_fields_go_out_as_written() -> Result<()> {
        let client = client_at(Url::parse("http://broker.test/ngsi-ld/v1/")?)?;
        let written = exchange(
            &client,
            Method::POST,
            "entityOperations/query?limit=5",
            &[("Content-Type", "application/ld+json"), ("Accept", "application/json")],
            Some(r#"{ "type": "Query" }"#),
        )?;
        let request = written.as_request();
        expect_that!(request.method(), eq(&Method::POST));
        expect_that!(
            request.uri().path_and_query().map(PathAndQuery::as_str),
            some(ends_with("/entityOperations/query?limit=5"))
        );
        expect_that!(
            request.headers().get(CONTENT_TYPE).and_then(|value| value.to_str().ok()),
            some(eq("application/ld+json"))
        );
        expect_that!(request.headers().get(ACCEPT), some(anything()));
        expect_that!(request.body(), eq(&Bytes::from_static(br#"{ "type": "Query" }"#)));
        Ok(())
    }

    #[gtest]
    fn a_call_without_a_body_or_fields_names_neither() -> Result<()> {
        let client = client_at(Url::parse("http://broker.test/ngsi-ld/v1/")?)?;
        let written = exchange(&client, Method::GET, "entities", &[], None)?;
        let request = written.as_request();
        expect_that!(request.headers().get(CONTENT_TYPE), none());
        verify_that!(request.body().is_empty(), eq(true))
    }
}
