//! The problem document a request whose method a resource does not serve is
//! answered with, and the registration that answers it.

use crate::problem_response::ProblemResponse;
use andromeda_failure_classification::classified_report::ClassifiedReport;
use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
use axum::{Router, extract::OriginalUri, http::Method};
use thiserror::Error;

/// A request method the addressed resource does not serve.
#[derive(Debug, Error)]
#[error("{0} is not defined for this resource")]
struct UnservedMethod(Method);

/// Register the wrong-method document as the method-not-allowed fallback of an
/// assembled router, so every resource the router serves answers a method it
/// does not serve with the document and the `Allow` header Axum computes for
/// that resource (RFC 9110, clause 15.5.6).
pub fn registered_on(router: Router) -> Router {
    router.method_not_allowed_fallback(problem_document)
}

/// The clause 5.5.3 document for a request whose method the resource does not
/// serve, with `instance` the request target and a `detail` naming the method.
///
/// The fallback runs inside a nested router, so the target is read from the
/// `OriginalUri` the outermost router recorded, not from the routed URI whose
/// mount prefix the nesting stripped.
async fn problem_document(method: Method, OriginalUri(uri): OriginalUri) -> ProblemResponse {
    ProblemResponse::classified(
        ClassifiedReport::public(NgsiLdErrorType::MethodNotAllowed, UnservedMethod(method).to_string()),
        uri,
    )
}

#[cfg(test)]
mod tests {
    use crate::method_not_allowed::registered_on;
    use axum::{
        Router,
        body::{Body, to_bytes},
        http::{
            Method,
            Request,
            StatusCode,
            header::{CONTENT_LENGTH, CONTENT_TYPE},
        },
        routing::post,
    };
    use std::error::Error;
    use tower::ServiceExt;

    /// The fallback registered the way an assembled router registers it: a
    /// resource serving one method, every other method answered by the
    /// fallback.
    fn probe_router() -> Router {
        registered_on(Router::new().route("/probe", post(|| async { StatusCode::OK })))
    }

    #[tokio::test]
    async fn a_wrong_method_request_gets_the_document_and_the_allow_header() -> Result<(), Box<dyn Error>> {
        let response = probe_router()
            .oneshot(Request::builder().method(Method::DELETE).uri("/probe").body(Body::empty())?)
            .await?;
        pretty_assertions::assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
        let allow = response.headers().get("allow").and_then(|value| value.to_str().ok());
        assert!(allow.is_some_and(|allow| allow.contains("POST") && !allow.contains("DELETE")), "{allow:?}");
        pretty_assertions::assert_eq!(
            response.headers().get(CONTENT_TYPE).and_then(|value| value.to_str().ok()),
            Some("application/json")
        );
        let body = to_bytes(response.into_body(), usize::MAX).await?;
        let document: serde_json::Value = serde_json::from_slice(&body)?;
        pretty_assertions::assert_eq!(document["type"], "https://uri.etsi.org/ngsi-ld/errors/MethodNotAllowed");
        pretty_assertions::assert_eq!(document["title"], "Method not allowed");
        pretty_assertions::assert_eq!(document["status"], 405);
        pretty_assertions::assert_eq!(document["detail"], "DELETE is not defined for this resource");
        pretty_assertions::assert_eq!(document["instance"], "/probe");
        Ok(())
    }

    /// A `HEAD` on the resource is answered the document with no body: the
    /// declared length names the document, the body itself is empty, as a
    /// `HEAD` response requires (RFC 9110, clause 9.3.2).
    #[tokio::test]
    async fn a_head_request_gets_the_document_without_a_body() -> Result<(), Box<dyn Error>> {
        let response = probe_router()
            .oneshot(Request::builder().method(Method::HEAD).uri("/probe").body(Body::empty())?)
            .await?;
        pretty_assertions::assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
        let length = response
            .headers()
            .get(CONTENT_LENGTH)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<usize>().ok());
        assert!(length.is_some_and(|length| length > 0), "a declared document length, found {length:?}");
        let body = to_bytes(response.into_body(), usize::MAX).await?;
        pretty_assertions::assert_eq!(body.len(), 0);
        Ok(())
    }
}
