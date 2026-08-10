//! Propagation of a valid tenant request field onto the corresponding response.

use andromeda_http_headers::ngsild_tenant::NgsiLdTenant;
use axum::{
    body::Body,
    http::{Request, Response},
    middleware::Next,
};
use headers::HeaderMapExt;

/// Echo a valid `NGSILD-Tenant` request field on the response.
pub async fn echo_tenant_header(request: Request<Body>, next: Next) -> Response<Body> {
    let tenant = request.headers().typed_try_get::<NgsiLdTenant>().ok().flatten();
    let mut response = next.run(request).await;
    if let Some(tenant) = tenant {
        response.headers_mut().typed_insert(tenant);
    }
    response
}
