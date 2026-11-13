//! Refusing Snapshot scope on the route families outside the scoped APIs.
//!
//! Clause 5.5.15 permits on a Snapshot the operations of the Core API and the Temporal API alone. Every other route
//! family takes this guard as a layer, so a request naming a Snapshot there is refused before its handler runs instead
//! of reading or changing the live tenant.

use crate::addressed_uri::addressed_request_uri;
use andromeda_http_headers::ngsild_snapshot::NgsiLdSnapshot;
use andromeda_http_response::request_refusal::RequestRefusal;
use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
use axum::{
    extract::Request,
    middleware::Next,
    response::{IntoResponse, Response},
};
use headers::{Header, HeaderMapExt};
use thiserror::Error;

/// A request naming a Snapshot to an operation clause 5.5.15 does not permit on one.
#[derive(Debug, Error)]
#[error("ngsild-snapshot is supported only by the core, temporal, and entity-subscription operations")]
struct SnapshotScopeRefused;

impl RequestRefusal for SnapshotScopeRefused {
    fn error_type(&self) -> NgsiLdErrorType {
        NgsiLdErrorType::OperationNotSupported
    }
}

/// Refuse a request carrying `NGSILD-Snapshot`, whatever its value, with `OperationNotSupported`.
///
/// A valid value is echoed on the refusal, since clause 6.3.22 puts the header on the response to every request that
/// carries it.
pub async fn reject_snapshot_scope(request: Request, next: Next) -> Response {
    if !request.headers().contains_key(NgsiLdSnapshot::name()) {
        return next.run(request).await;
    }
    let named = request.headers().typed_try_get::<NgsiLdSnapshot>().ok().flatten();
    let mut response = SnapshotScopeRefused.into_problem(addressed_request_uri(&request)).into_response();
    if let Some(snapshot) = named {
        response.headers_mut().typed_insert(snapshot);
    }
    response
}

#[cfg(test)]
mod tests {
    use crate::snapshot_scope_guard::reject_snapshot_scope;
    use axum::{
        Router,
        body::{Body, to_bytes},
        http::{Request, StatusCode},
        middleware::from_fn,
        response::Response,
        routing::get,
    };
    use serde_json::Value;
    use std::error::Error;
    use tower::ServiceExt;

    async fn answer(snapshot: Option<&str>) -> Result<Response, Box<dyn Error>> {
        let router = Router::new()
            .route("/health", get(|| async { StatusCode::OK }))
            .layer(from_fn(reject_snapshot_scope));
        let mut request = Request::builder().uri("/health");
        if let Some(snapshot) = snapshot {
            request = request.header("NGSILD-Snapshot", snapshot);
        }
        Ok(router.oneshot(request.body(Body::empty())?).await?)
    }

    async fn problem(response: Response) -> Result<Value, Box<dyn Error>> {
        Ok(serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await?)?)
    }

    #[tokio::test]
    async fn a_snapshot_header_is_refused_as_an_operation_not_supported_naming_the_scoped_families() -> Result<(), Box<dyn Error>> {
        let response = answer(Some("urn:ngsi-ld:Snapshot:1")).await?;
        pretty_assertions::assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let document = problem(response).await?;
        pretty_assertions::assert_eq!(document["type"], "https://uri.etsi.org/ngsi-ld/errors/OperationNotSupported");
        pretty_assertions::assert_eq!(
            document["detail"],
            "ngsild-snapshot is supported only by the core, temporal, and entity-subscription operations"
        );
        pretty_assertions::assert_eq!(document["instance"], "/health");
        Ok(())
    }

    /// Clause 6.3.22: the header a request carries is present on its response.
    #[tokio::test]
    async fn the_refusal_carries_the_snapshot_the_request_named() -> Result<(), Box<dyn Error>> {
        let response = answer(Some("urn:ngsi-ld:Snapshot:1")).await?;
        let echoed = response.headers().get("ngsild-snapshot").map(|value| value.to_str()).transpose()?;
        pretty_assertions::assert_eq!(echoed, Some("urn:ngsi-ld:Snapshot:1"));
        Ok(())
    }

    #[tokio::test]
    async fn a_snapshot_header_that_is_no_uri_is_refused_and_not_echoed() -> Result<(), Box<dyn Error>> {
        let response = answer(Some("not a URI")).await?;
        pretty_assertions::assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        assert!(!response.headers().contains_key("ngsild-snapshot"), "{:?}", response.headers());
        Ok(())
    }

    #[tokio::test]
    async fn a_request_without_a_snapshot_header_reaches_its_route() -> Result<(), Box<dyn Error>> {
        pretty_assertions::assert_eq!(answer(None).await?.status(), StatusCode::OK);
        Ok(())
    }
}
