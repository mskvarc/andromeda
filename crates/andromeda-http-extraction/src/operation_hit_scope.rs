//! Bound one NGSI-LD request as a single clause 5.13.3.5 counting operation.
//!
//! A single request resolves the same `@context` several times internally: a
//! write is screened (clause 5.5.4) and then expanded again for storage. Without
//! a scope each internal resolution would be a separate `numberOfHits`, so the
//! count would report the broker's plumbing rather than the client-visible
//! "number of times the @context was found in the storage". The scope collapses
//! them: [`andromeda_jsonld::operation_hit_scope`] counts the first access of
//! each context URL within it and folds the rest in.

use andromeda_jsonld::operation_hit_scope::run_in_operation_scope;
use axum::{extract::Request, middleware::Next, response::Response};

/// Run the request within one operation scope so its context resolutions count
/// once per URL (clause 5.13.3.5).
pub async fn scope_operation_hits(request: Request, next: Next) -> Response {
    run_in_operation_scope(next.run(request)).await
}
