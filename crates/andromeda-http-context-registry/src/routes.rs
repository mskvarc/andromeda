//! Typed, state-complete inventory of Context Registry routes.

use crate::{
    handler,
    served_document::vary_on_tenant,
    state::{ContextRegistryRouteState, ContextRegistryStateResolver},
};
use axum::{
    Router,
    handler::Handler,
    middleware::map_response,
    routing::{get, post},
};
use std::sync::Arc;

/// The complete Context Registry route fragment.
pub struct ContextRegistryRoutes {
    router: Router,
}

impl ContextRegistryRoutes {
    /// Construct all `/jsonldContexts` routes with focused state.
    pub fn new(resolver: Arc<dyn ContextRegistryStateResolver>) -> ContextRegistryRoutes {
        let context_routes = Router::new()
            .route("/", post(handler::create).get(handler::query))
            .route("/{context_id}", get(handler::get.layer(map_response(vary_on_tenant))).delete(handler::delete));
        let router = Router::new()
            .nest("/jsonldContexts", context_routes)
            .with_state(ContextRegistryRouteState::new(resolver));
        ContextRegistryRoutes { router }
    }

    /// Consume the state-complete route fragment.
    pub fn into_router(self) -> Router {
        self.router
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod serve_tests;

#[cfg(test)]
mod tests {
    use crate::{
        routes::ContextRegistryRoutes,
        state::{ContextRegistryRouteServices, ContextRegistryStateResolver},
    };
    use andromeda_tenancy::{creation_policy::TenantCreationPolicy, error::TenancyError, tenant_ref::TenantRef};
    use axum::{
        body::{Body, to_bytes},
        http::{Error as HttpError, Method, Request, StatusCode, header::CONTENT_LENGTH},
    };
    use futures::future::BoxFuture;
    use pretty_assertions::{assert_eq, assert_ne};
    use std::sync::Arc;
    use tower::ServiceExt;

    struct RejectingResolver;

    impl ContextRegistryStateResolver for RejectingResolver {
        fn resolve<'a>(
            &'a self,
            _tenant: &'a TenantRef,
            _creation_policy: TenantCreationPolicy,
        ) -> BoxFuture<'a, Result<Arc<ContextRegistryRouteServices>, TenancyError>> {
            Box::pin(async { Err(TenancyError::MultiTenancyDisabled) })
        }
    }

    #[tokio::test]
    async fn owns_the_complete_context_registry_inventory() -> Result<(), HttpError> {
        let routes = ContextRegistryRoutes::new(Arc::new(RejectingResolver)).into_router();
        for (method, path) in [
            (Method::POST, "/jsonldContexts"),
            (Method::GET, "/jsonldContexts"),
            (Method::GET, "/jsonldContexts/context-1"),
            (Method::DELETE, "/jsonldContexts/context-1"),
        ] {
            let request = Request::builder()
                .method(method.clone())
                .uri(path)
                .header("content-type", "application/json")
                .body(Body::from("{}"))?;
            // A `Router`'s service error type is uninhabited, so the call has
            // no failure case to propagate.
            let response = match routes.clone().oneshot(request).await {
                Ok(response) => response,
                Err(never) => match never {},
            };
            assert_ne!(response.status(), StatusCode::NOT_FOUND, "{method} {path} was not routed");
            assert_ne!(response.status(), StatusCode::METHOD_NOT_ALLOWED, "{method} {path} lost its method");
        }
        Ok(())
    }

    /// Clause 6.3.4: "'Content-Length' HTTP header absence, shall result in
    /// just a 411 HTTP status code (without any payload body)". Creating a
    /// hosted `@context` (clause 5.13.2) is an HTTP POST with a payload body,
    /// so the precondition holds for it, and it holds before the addressed
    /// tenant is resolved.
    #[tokio::test]
    async fn creating_a_context_without_a_declared_length_is_the_bare_411() -> Result<(), HttpError> {
        let routes = ContextRegistryRoutes::new(Arc::new(RejectingResolver)).into_router();
        let request = Request::builder()
            .method(Method::POST)
            .uri("/jsonldContexts")
            .header("content-type", "application/json")
            .body(Body::from("{}"))?;

        // A `Router`'s service error type is uninhabited, so the call has no
        // failure case to propagate.
        let response = match routes.oneshot(request).await {
            Ok(response) => response,
            Err(never) => match never {},
        };

        assert_eq!(response.status(), StatusCode::LENGTH_REQUIRED);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap_or_default();
        assert!(body.is_empty(), "clause 6.3.4 forbids a payload body on a 411: {body:?}");
        Ok(())
    }

    /// Clause 6.3.4: "Unsupported Media Type, i.e. 'Content-Type' header is not
    /// 'application/json' nor 'application/ld+json', shall result in just a 415
    /// HTTP status code (without any payload body)". The precondition holds for
    /// creating a hosted `@context` before the addressed tenant is resolved.
    #[tokio::test]
    async fn creating_a_context_with_an_unsupported_media_type_is_the_bare_415() -> Result<(), HttpError> {
        for content_type in [Some("application/xml"), Some("text/plain"), None] {
            let routes = ContextRegistryRoutes::new(Arc::new(RejectingResolver)).into_router();
            let mut request = Request::builder().method(Method::POST).uri("/jsonldContexts").header(CONTENT_LENGTH, "2");
            if let Some(content_type) = content_type {
                request = request.header("content-type", content_type);
            }

            // A `Router`'s service error type is uninhabited, so the call has no
            // failure case to propagate.
            let response = match routes.oneshot(request.body(Body::from("{}"))?).await {
                Ok(response) => response,
                Err(never) => match never {},
            };

            assert_eq!(response.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE, "{content_type:?}");
            let body = to_bytes(response.into_body(), usize::MAX).await.unwrap_or_default();
            assert!(body.is_empty(), "clause 6.3.4 forbids a payload body on a 415: {body:?}");
        }
        Ok(())
    }

    /// A declared length lets the request through the precondition and on to
    /// the operation, which the rejecting resolver then refuses.
    #[tokio::test]
    async fn a_declared_length_carries_the_request_past_the_precondition() -> Result<(), HttpError> {
        let routes = ContextRegistryRoutes::new(Arc::new(RejectingResolver)).into_router();
        let request = Request::builder()
            .method(Method::POST)
            .uri("/jsonldContexts")
            .header("content-type", "application/json")
            .header(CONTENT_LENGTH, "2")
            .body(Body::from("{}"))?;

        let response = match routes.oneshot(request).await {
            Ok(response) => response,
            Err(never) => match never {},
        };

        assert_ne!(response.status(), StatusCode::LENGTH_REQUIRED);
        Ok(())
    }
}
