//! A plain JSON request body decoded into a typed value.
//!
//! The operations whose body is a small JSON document of Andromeda's own
//! rather than an NGSI-LD JSON-LD document -- toggling a tenant -- read it
//! here. The body is subject to the same clause 6.3.4
//! preconditions and the same size limit as every other payload, and each
//! refusal is a problem detail rather than the framework's plain text.

use crate::{
    addressed_uri::addressed_request_uri,
    content_length_precondition::declared_body_length,
    json_value_budget::refuse_values_past_budget,
    request_json_value_limit::RequestJsonValueLimit,
    request_payload::buffer_payload,
};
use andromeda_http_response::extraction_rejection::ExtractorRejection;
use axum::{
    extract::{FromRequest, Request},
    http::{HeaderMap, StatusCode},
};
use headers::HeaderMapExt;
use mediatype::{MediaTypeBuf, names};
use serde::de::DeserializeOwned;
use serde_json::error::Category;

/// A JSON request body decoded into `T`.
pub struct JsonPayload<T>(pub T);

impl<S, T> FromRequest<S> for JsonPayload<T>
where
    S: Send + Sync,
    T: DeserializeOwned,
{
    type Rejection = ExtractorRejection;

    async fn from_request(request: Request, state: &S) -> Result<JsonPayload<T>, ExtractorRejection> {
        let instance = addressed_request_uri(&request);
        // Clause 6.3.4 answers an absent `Content-Length` with a bare 411
        // before the `Content-Type` is looked at, and an unsupported media type
        // with "just a 415 HTTP status code (without any payload body)".
        // clone: each refusal below names the same request.
        let declared = declared_body_length(request.headers(), request.extensions()).map_err(|error| error.into_rejection(instance.clone()))?;
        if !announces_json(request.headers()) {
            return Err(ExtractorRejection::Empty(StatusCode::UNSUPPORTED_MEDIA_TYPE));
        }
        let value_limit = request.extensions().get::<RequestJsonValueLimit>().copied();
        // clone: a body that is read but does not decode still names the request.
        let body = buffer_payload(request, state, declared, instance.clone()).await?;
        // clone: a refused payload names the request, which a parse failure below names too.
        refuse_values_past_budget(&body, value_limit).map_err(|refusal| ExtractorRejection::Classified(refusal.report().with_instance(instance.clone())))?;
        serde_json::from_slice(&body).map(JsonPayload).map_err(|error| {
            let detail = format!("Invalid JSON: {error}");
            // Clause 5.5.4: a body that is not a JSON document is an invalid
            // request; a JSON document of the wrong shape is bad data.
            match error.classify() {
                Category::Data => ExtractorRejection::bad_request_data(detail, instance),
                Category::Io | Category::Syntax | Category::Eof => ExtractorRejection::invalid_request(detail, instance),
            }
        })
    }
}

/// Whether the request's `Content-Type` announces a JSON document:
/// `application/json` or an `application/*+json` type.
fn announces_json(headers: &HeaderMap) -> bool {
    let Some(content_type) = headers.typed_get::<headers::ContentType>() else {
        return false;
    };
    content_type
        .to_string()
        .parse::<MediaTypeBuf>()
        .is_ok_and(|media_type| media_type.ty() == names::APPLICATION && (media_type.subty() == names::JSON || media_type.suffix() == Some(names::JSON)))
}

#[cfg(test)]
mod tests {
    use crate::{json_payload::JsonPayload, request_json_value_limit::RequestJsonValueLimit};
    use andromeda_http_response::extraction_rejection::ExtractorRejection;
    use andromeda_ngsi_ld_number::json_value::value_census::JsonValueLimit;
    use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
    use axum::{
        body::Body,
        extract::FromRequest,
        http::{
            Request,
            StatusCode,
            header::{CONTENT_LENGTH, CONTENT_TYPE},
        },
    };
    use serde::Deserialize;
    use std::num::NonZeroUsize;

    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Toggle {
        is_active: bool,
    }

    /// A request carrying `body` under `content_type`, declaring its length.
    fn request(content_type: &str, body: &'static str) -> Request<Body> {
        Request::patch("/ngsi-ld/v1/tenants/acme")
            .header(CONTENT_TYPE, content_type)
            .header(CONTENT_LENGTH, body.len())
            .body(Body::from(body))
            .expect("a valid request")
    }

    async fn extract(request: Request<Body>) -> Result<JsonPayload<Toggle>, ExtractorRejection> {
        JsonPayload::<Toggle>::from_request(request, &()).await
    }

    #[tokio::test]
    async fn a_json_body_of_the_expected_shape_is_decoded() {
        for content_type in ["application/json", "application/merge-patch+json", "application/json; charset=utf-8"] {
            let JsonPayload(toggle) = extract(request(content_type, r#"{"isActive":false}"#)).await.expect("a decodable body");
            assert!(!toggle.is_active, "{content_type}");
        }
    }

    /// Clause 5.5.4: a body that is not a JSON document is `InvalidRequest`.
    #[tokio::test]
    async fn a_body_that_is_not_json_is_an_invalid_request() {
        assert!(matches!(
            extract(request("application/json", "{")).await,
            Err(ExtractorRejection::InvalidRequest { .. })
        ));
    }

    /// A JSON document of the wrong shape is `BadRequestData`.
    #[tokio::test]
    async fn a_json_document_of_the_wrong_shape_is_bad_request_data() {
        assert!(matches!(
            extract(request("application/json", r#"{"isActive":"no"}"#)).await,
            Err(ExtractorRejection::BadRequestData { .. })
        ));
    }

    /// Clause 6.3.4: an unsupported media type is just a 415.
    #[tokio::test]
    async fn a_body_that_is_not_announced_as_json_is_the_bare_415() {
        for content_type in ["text/plain", "application/xml"] {
            match extract(request(content_type, r#"{"isActive":true}"#)).await {
                Err(ExtractorRejection::Empty(status)) => pretty_assertions::assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE),
                Ok(_) | Err(_) => panic!("{content_type} must be the bare 415"),
            }
        }
    }

    /// Clause 6.3.4: an absent `Content-Length` is just a 411, checked first.
    #[tokio::test]
    async fn a_body_declaring_no_length_is_the_bare_411() {
        let request = Request::patch("/ngsi-ld/v1/tenants/acme")
            .header(CONTENT_TYPE, "text/plain")
            .body(Body::from("{}"))
            .expect("a valid request");

        match extract(request).await {
            Err(ExtractorRejection::Empty(status)) => pretty_assertions::assert_eq!(status, StatusCode::LENGTH_REQUIRED),
            Ok(_) | Err(_) => panic!("an undeclared length must be the bare 411"),
        }
    }

    /// Clause 6.3.2: a plain JSON payload holding more values than the
    /// published limit is the 413 "Request Entity too large", before it is
    /// decoded.
    #[tokio::test]
    async fn a_plain_json_payload_past_the_limit_is_refused() {
        let mut over = request("application/json", r#"{"isActive":true,"extra":[1,2,3]}"#);
        over.extensions_mut().insert(RequestJsonValueLimit::new(JsonValueLimit::new(NonZeroUsize::MIN)));

        match extract(over).await {
            Err(ExtractorRejection::Classified(refusal)) => {
                pretty_assertions::assert_eq!(refusal.problem().error_type(), NgsiLdErrorType::RequestEntityTooLarge);
                assert!(refusal.problem().detail().contains("1 JSON values"), "{}", refusal.problem().detail());
            }
            Ok(_) | Err(_) => panic!("a payload of six values past a limit of one must be refused"),
        }
    }

    /// A payload within the published limit is decoded as without one.
    #[tokio::test]
    async fn a_plain_json_payload_within_the_limit_is_decoded() {
        let mut within = request("application/json", r#"{"isActive":true}"#);
        within
            .extensions_mut()
            .insert(RequestJsonValueLimit::new(JsonValueLimit::new(NonZeroUsize::MIN.saturating_add(1))));

        let JsonPayload(toggle) = extract(within).await.expect("two values are within a limit of two");
        assert!(toggle.is_active);
    }
}
