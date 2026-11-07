//! The clause 6.3.4 preconditions of a body-taking operation, as one extractor.
//!
//! ETSI GS CIM 009 v1.9.1 clause 6.3.4 checks the `Content-Length` and the
//! `Content-Type` of an HTTP POST, PATCH or PUT request, and answers the first
//! unmet precondition with a bare status. Both are properties of the incoming
//! request alone, so a route lists [`BodyPreconditions`] ahead of every
//! extractor that can refuse it for another reason, and they hold whether or not
//! the addressed tenant resolves.

use crate::{
    addressed_uri::addressed_uri,
    content_kind::admit_content_kind,
    content_length_precondition::{DeclaredBodyLength, declared_body_length},
};
use andromeda_http_response::extraction_rejection::ExtractorRejection;
use axum::{extract::FromRequestParts, http::request::Parts};
use std::future::{Future, ready};

/// A request that passed the clause 6.3.4 `Content-Length` and `Content-Type`
/// preconditions, in the clause's order.
#[derive(Clone, Copy, Debug)]
pub struct BodyPreconditions {
    /// The payload length the request declared.
    pub declared: DeclaredBodyLength,
}

impl<S> FromRequestParts<S> for BodyPreconditions
where
    S: Send + Sync,
{
    type Rejection = ExtractorRejection;

    /// Clause 6.3.4: an absent `Content-Length` is the bare 411 and an
    /// unsupported `Content-Type` the bare 415; the length is checked first, so
    /// a request that meets neither is answered by the length it did not
    /// declare.
    fn from_request_parts(parts: &mut Parts, _state: &S) -> impl Future<Output = Result<BodyPreconditions, Self::Rejection>> {
        ready(
            declared_body_length(&parts.headers, &parts.extensions)
                .map_err(|error| error.into_rejection(addressed_uri(parts)))
                .and_then(|declared| admit_content_kind(&parts.headers, &parts.method).map(|_| BodyPreconditions { declared })),
        )
    }
}

#[cfg(test)]
mod tests {
    use crate::body_preconditions::BodyPreconditions;
    use andromeda_http_response::extraction_rejection::ExtractorRejection;
    use axum::{
        extract::FromRequestParts,
        http::{
            Method,
            Request,
            StatusCode,
            header::{CONTENT_LENGTH, CONTENT_TYPE},
        },
    };

    /// The preconditions' verdict on a POST declaring `length` and announcing
    /// `content_type`, each when present.
    async fn verdict(length: Option<&str>, content_type: Option<&str>) -> Result<BodyPreconditions, ExtractorRejection> {
        let mut request = Request::builder().method(Method::POST).uri("/ngsi-ld/v1/jsonldContexts");
        if let Some(length) = length {
            request = request.header(CONTENT_LENGTH, length);
        }
        if let Some(content_type) = content_type {
            request = request.header(CONTENT_TYPE, content_type);
        }
        let (mut parts, ()) = request.body(()).expect("a valid request").into_parts();
        BodyPreconditions::from_request_parts(&mut parts, &()).await
    }

    /// The bare status a verdict refuses with.
    fn bare_status(verdict: &Result<BodyPreconditions, ExtractorRejection>) -> StatusCode {
        match verdict {
            Err(ExtractorRejection::Empty(status)) => *status,
            Ok(_) | Err(_) => panic!("clause 6.3.4 requires a bare status"),
        }
    }

    /// Clause 6.3.4: an unsupported or absent `Content-Type` is "just a 415
    /// HTTP status code (without any payload body)".
    #[tokio::test]
    async fn an_unsupported_or_absent_media_type_is_the_bare_415() {
        for content_type in [Some("application/xml"), Some("text/plain"), Some("not a media type"), None] {
            pretty_assertions::assert_eq!(
                bare_status(&verdict(Some("2"), content_type).await),
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "{content_type:?}"
            );
        }
    }

    /// Clause 6.3.4: an absent `Content-Length` is the bare 411, and is
    /// checked before the `Content-Type`.
    #[tokio::test]
    async fn an_absent_length_is_the_bare_411_even_beside_an_unsupported_media_type() {
        for content_type in [Some("application/xml"), Some("application/json"), None] {
            pretty_assertions::assert_eq!(bare_status(&verdict(None, content_type).await), StatusCode::LENGTH_REQUIRED, "{content_type:?}");
        }
    }

    /// A request meeting both preconditions is admitted with its length.
    #[tokio::test]
    async fn a_declared_length_and_a_supported_media_type_are_admitted() {
        for content_type in ["application/json", "application/ld+json"] {
            let admitted = verdict(Some("17"), Some(content_type)).await.expect("both preconditions are met");
            pretty_assertions::assert_eq!(admitted.declared.bytes().get(), 17, "{content_type}");
        }
    }
}
