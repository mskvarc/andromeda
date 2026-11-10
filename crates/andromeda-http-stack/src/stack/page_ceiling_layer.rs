//! The refusal of a collection page that renders larger than the ceiling.
//!
//! A collection response carries the size of its rendered body, so the ceiling
//! is held against it here, once, whichever collection the handler answered.
//! Only a collection is held to it: a single resource, a batch result or an
//! error carries no size and is never refused by it.

use andromeda_http_extraction::addressed_uri::addressed_request_uri;
use andromeda_http_response::{
    page_ceiling::{PageByteCeiling, RenderedPageSize},
    problem_response::ProblemResponse,
};
use axum::{
    extract::{Request, State},
    middleware::Next,
    response::{IntoResponse, Response},
};

/// Run the rest of the stack, answering `TooManyResults` (clause 5.5.6) in place
/// of a collection page that renders to more bytes than `ceiling` admits.
pub(super) async fn refuse_oversized_page(State(ceiling): State<PageByteCeiling>, request: Request, next: Next) -> Response {
    let instance = addressed_request_uri(&request);
    let response = next.run(request).await;
    match response.extensions().get::<RenderedPageSize>().copied().map(|size| ceiling.admit(size)) {
        Some(Err(refused)) => ProblemResponse::classified(refused.report(), instance).into_response(),
        Some(Ok(())) | None => response,
    }
}

#[cfg(test)]
mod tests {
    use crate::stack::page_ceiling_layer::refuse_oversized_page;
    use andromeda_http_response::page_ceiling::{PageByteCeiling, RenderedPageSize};
    use axum::{
        Router,
        body::{Body, to_bytes},
        extract::Request,
        http::{StatusCode, header::CONTENT_TYPE},
        middleware::from_fn_with_state,
        response::{IntoResponse, Response},
        routing::get,
    };
    use serde_json::Value;
    use std::error::Error;
    use tower::ServiceExt;

    /// A collection response whose body is `bytes` long and says so.
    fn page_of(bytes: usize) -> Response {
        let mut response = vec![b'x'; bytes].into_response();
        response.extensions_mut().insert(RenderedPageSize::new(bytes));
        response
    }

    fn router(ceiling: usize) -> Router {
        Router::new()
            .route("/ngsi-ld/v1/entities", get(|| async { page_of(100) }))
            .route("/ngsi-ld/v1/entities/big", get(|| async { vec![b'x'; 1000].into_response() }))
            .layer(from_fn_with_state(PageByteCeiling::new(ceiling), refuse_oversized_page))
    }

    async fn answer(router: Router, uri: &str) -> Result<Response, Box<dyn Error>> {
        Ok(router.oneshot(Request::get(uri).body(Body::empty())?).await?)
    }

    #[tokio::test]
    async fn a_page_exactly_at_the_ceiling_is_served() -> Result<(), Box<dyn Error>> {
        let response = answer(router(100), "/ngsi-ld/v1/entities").await?;
        pretty_assertions::assert_eq!(response.status(), StatusCode::OK);
        pretty_assertions::assert_eq!(to_bytes(response.into_body(), usize::MAX).await?.len(), 100);
        Ok(())
    }

    #[tokio::test]
    async fn a_page_one_byte_over_the_ceiling_is_a_too_many_results_problem() -> Result<(), Box<dyn Error>> {
        let response = answer(router(99), "/ngsi-ld/v1/entities?type=Room").await?;
        pretty_assertions::assert_eq!(response.status(), StatusCode::FORBIDDEN);
        pretty_assertions::assert_eq!(
            response.headers().get(CONTENT_TYPE).and_then(|value| value.to_str().ok()),
            Some("application/json")
        );
        let problem: Value = serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await?)?;
        pretty_assertions::assert_eq!(problem["type"], "https://uri.etsi.org/ngsi-ld/errors/TooManyResults");
        pretty_assertions::assert_eq!(problem["status"], 403);
        pretty_assertions::assert_eq!(problem["instance"], "/ngsi-ld/v1/entities?type=Room");
        pretty_assertions::assert_eq!(
            problem["detail"],
            "the page renders to 100 bytes, more than the 99 bytes one page may hold; ask for a smaller limit and follow the next page pointer"
        );
        Ok(())
    }

    /// Only a collection page is a page: a single resource over the ceiling is
    /// served.
    #[tokio::test]
    async fn a_response_that_is_no_collection_page_is_never_refused() -> Result<(), Box<dyn Error>> {
        let response = answer(router(10), "/ngsi-ld/v1/entities/big").await?;
        pretty_assertions::assert_eq!(response.status(), StatusCode::OK);
        pretty_assertions::assert_eq!(to_bytes(response.into_body(), usize::MAX).await?.len(), 1000);
        Ok(())
    }
}
