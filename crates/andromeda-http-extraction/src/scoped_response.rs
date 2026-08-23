//! Echo validated tenant and Snapshot scope headers on every route response.

use andromeda_http_headers::{ngsild_snapshot::NgsiLdSnapshot, ngsild_tenant::NgsiLdTenant};
use axum::{extract::Request, middleware::Next, response::Response};
use headers::HeaderMapExt;

/// Echo valid `NGSILD-Tenant` and `NGSILD-Snapshot` request values on success and error responses.
pub async fn echo_scope_headers(request: Request, next: Next) -> Response {
    let tenant = request.headers().typed_try_get::<NgsiLdTenant>().ok().flatten();
    let snapshot = request.headers().typed_try_get::<NgsiLdSnapshot>().ok().flatten();
    let mut response = next.run(request).await;
    if let Some(tenant) = tenant {
        response.headers_mut().typed_insert(tenant);
    }
    if let Some(snapshot) = snapshot {
        response.headers_mut().typed_insert(snapshot);
    }
    response
}
