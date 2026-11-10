//! Refusing Snapshot scope on route families outside the scoped APIs.

use crate::addressed_uri::addressed_request_uri;
use andromeda_failure_classification::classified_report::ClassifiedReport;
use andromeda_http_headers::ngsild_snapshot::NgsiLdSnapshot;
use andromeda_http_response::problem_response::ProblemResponse;
use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
use axum::{
    extract::Request,
    middleware::Next,
    response::{IntoResponse, Response},
};
use headers::Header;

/// Reject `NGSILD-Snapshot` on a route family outside Core, Temporal, and entity-subscription APIs.
pub async fn reject_snapshot_scope(request: Request, next: Next) -> Response {
    if request.headers().contains_key(NgsiLdSnapshot::name()) {
        return ProblemResponse::classified(
            ClassifiedReport::public(
                NgsiLdErrorType::OperationNotSupported,
                "ngsild-snapshot is supported only by the core, temporal, and entity-subscription operations",
            ),
            addressed_request_uri(&request),
        )
        .into_response();
    }
    next.run(request).await
}

#[cfg(test)]
mod tests {
    use crate::snapshot_scope_guard::reject_snapshot_scope;
    use axum::{
        Router,
        body::Body,
        http::{Request, StatusCode},
        middleware::from_fn,
        routing::get,
    };
    use tower::ServiceExt;

    #[tokio::test]
    async fn a_snapshot_header_is_rejected_outside_the_scoped_api_families() {
        let router = Router::new()
            .route("/health", get(|| async { StatusCode::OK }))
            .layer(from_fn(reject_snapshot_scope));
        let request = Request::builder()
            .uri("/health")
            .header("NGSILD-Snapshot", "urn:ngsi-ld:Snapshot:1")
            .body(Body::empty())
            .expect("a valid request");
        let response = router.oneshot(request).await.expect("the router answers");
        pretty_assertions::assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }
}
