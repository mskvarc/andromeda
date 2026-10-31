//! A request whose JSON body goes out as the test writes it, for a body the typed model has no place for (a fragment
//! without `type`, a member the model refuses) or whose spelling is the subject.

use crate::wire::{WireError, content_type, request_to};
use bytes::Bytes;
use http::Method;
use ngsi_ld::{
    client::{Answering, Client, Transport},
    http::UncheckedRequest,
    model::APPLICATION_JSON,
};

/// A request with `method` to `path` (relative to the API root, with its query) carrying `body` byte for byte under
/// `Content-Type: application/json`, and no `Accept`.
///
/// # Errors
///
/// Returns [`WireError`] when the target is not a URI.
pub fn json_request_to<T: Transport, M: Answering>(client: &Client<T, M>, method: Method, path: &str, body: &str) -> Result<UncheckedRequest, WireError> {
    Ok(content_type(request_to(client, method, path)?, &APPLICATION_JSON)
        .map_err(WireError::MediaType)?
        .body(Bytes::copy_from_slice(body.as_bytes())))
}

#[cfg(test)]
mod tests {
    use crate::{broker::client_at, written_json::json_request_to};
    use bytes::Bytes;
    use googletest::prelude::*;
    use http::{
        Method,
        header::{ACCEPT, CONTENT_TYPE},
    };
    use url::Url;

    #[gtest]
    fn the_body_goes_out_as_written_under_the_json_media_type() -> Result<()> {
        let client = client_at(Url::parse("http://broker.test/ngsi-ld/v1/")?)?;
        let written = json_request_to(&client, Method::PATCH, "entities/urn:ngsi-ld:A:1/attrs/speed", r#"{ "value":  1 }"#)?;
        let request = written.as_request();
        expect_that!(request.method(), eq(&Method::PATCH));
        expect_that!(request.uri().path(), ends_with("/entities/urn:ngsi-ld:A:1/attrs/speed"));
        expect_that!(
            request.headers().get(CONTENT_TYPE).and_then(|value| value.to_str().ok()),
            some(eq("application/json"))
        );
        expect_that!(request.headers().get(ACCEPT), none());
        expect_that!(request.body().as_ref(), eq(&Bytes::from_static(br#"{ "value":  1 }"#)));
        Ok(())
    }
}
