//! Refusing provisioning requests targeted at a Snapshot.
//!
//! Clause 5.5.15 permits every Core and Temporal API operation on a Snapshot,
//! applied "only to information related to the specific Snapshot". This
//! broker serves the consumption operations from a Snapshot's frozen data but
//! has no provisioning path into one, so a write carrying `NGSILD-Snapshot`
//! cannot be honoured: silently applying it to the live context would violate
//! the same clause. It is refused as `OperationNotSupported` (clause 6.3.2)
//! instead, which is the honest answer for an operation the implementation
//! does not support on that target.

use andromeda_http_headers::ngsild_snapshot::NgsiLdSnapshot;
use andromeda_http_response::{classified_report::ClassifiedReport, problem_response::ProblemResponse};
use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
use axum::{
    extract::{OriginalUri, Request},
    http::{Method, Uri},
    middleware::Next,
    response::{IntoResponse, Response},
};
use headers::{Header, HeaderMapExt};

/// Axum middleware refusing a non-`GET` request that names a Snapshot.
///
/// Consumption over `POST` (`/entityOperations/query` and its temporal
/// counterpart) must not sit behind this layer: the route fragments attach it
/// to their provisioning routes only, and `GET` passes through so every read
/// keeps its Snapshot scope.
pub async fn reject_snapshot_scoped_writes(request: Request, next: Next) -> Response {
    if request.method() != Method::GET && request.headers().typed_get::<NgsiLdSnapshot>().is_some() {
        let instance = request
            .extensions()
            .get::<OriginalUri>()
            .map_or_else(|| request.uri().clone(), |original| original.0.clone());
        return snapshot_write_unsupported(instance).into_response();
    }
    next.run(request).await
}

/// Reject `NGSILD-Snapshot` on a route family outside Core, Temporal, and entity-subscription APIs.
pub async fn reject_snapshot_scope(request: Request, next: Next) -> Response {
    if request.headers().contains_key(NgsiLdSnapshot::name()) {
        let instance = request
            .extensions()
            .get::<OriginalUri>()
            .map_or_else(|| request.uri().clone(), |original| original.0.clone());
        return ClassifiedReport::public(
            NgsiLdErrorType::OperationNotSupported,
            "NGSILD-Snapshot is supported only by Core, Temporal, and entity-subscription operations",
        )
        .with_instance(instance)
        .into_response();
    }
    next.run(request).await
}

/// The `OperationNotSupported` problem a Snapshot-scoped write answers with.
fn snapshot_write_unsupported(instance: Uri) -> ProblemResponse {
    ClassifiedReport::public(
        NgsiLdErrorType::OperationNotSupported,
        "provisioning operations on a Snapshot are not supported by this broker; remove the NGSILD-Snapshot header to write to the live context",
    )
    .with_instance(instance)
}

#[cfg(test)]
mod tests {
    use crate::snapshot_write_guard::reject_snapshot_scoped_writes;
    use axum::{
        Router,
        body::Body,
        http::{Method, Request, StatusCode},
        middleware::from_fn,
        routing::{get, post},
    };
    use tower::ServiceExt;

    fn router() -> Router {
        Router::new()
            .route("/entities", post(|| async { StatusCode::CREATED }).get(|| async { StatusCode::OK }))
            .layer(from_fn(reject_snapshot_scoped_writes))
            .route("/entityOperations/query", post(|| async { StatusCode::OK }))
            .route("/health", get(|| async { StatusCode::OK }))
    }

    async fn status_of(method: Method, path: &str, snapshot: Option<&str>) -> StatusCode {
        let mut builder = Request::builder().method(method).uri(path);
        if let Some(snapshot) = snapshot {
            builder = builder.header("NGSILD-Snapshot", snapshot);
        }
        let request = builder.body(Body::empty()).expect("a valid request");
        router().oneshot(request).await.expect("the router answers").status()
    }

    const SNAPSHOT: &str = "urn:ngsi-ld:Snapshot:1";

    #[tokio::test]
    async fn a_snapshot_scoped_write_is_refused_as_operation_not_supported() {
        pretty_assertions::assert_eq!(status_of(Method::POST, "/entities", Some(SNAPSHOT)).await, StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn a_write_without_a_snapshot_passes_through() {
        pretty_assertions::assert_eq!(status_of(Method::POST, "/entities", None).await, StatusCode::CREATED);
    }

    #[tokio::test]
    async fn a_snapshot_scoped_read_passes_through() {
        pretty_assertions::assert_eq!(status_of(Method::GET, "/entities", Some(SNAPSHOT)).await, StatusCode::OK);
    }

    #[tokio::test]
    async fn a_route_outside_the_layer_is_not_guarded() {
        pretty_assertions::assert_eq!(status_of(Method::POST, "/entityOperations/query", Some(SNAPSHOT)).await, StatusCode::OK);
    }
}
