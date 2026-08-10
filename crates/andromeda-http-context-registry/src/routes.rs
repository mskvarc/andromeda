//! Typed, state-complete inventory of Context Registry routes.

use crate::{
    handler,
    state::{ContextRegistryRouteState, ContextRegistryStateResolver},
};
use axum::{
    Router,
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
            .route("/{context_id}", get(handler::get).delete(handler::delete));
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
mod tests {
    use crate::state::{ContextRegistryRouteServices, ContextRegistryStateResolver};
    use andromeda_tenancy::{creation_policy::TenantCreationPolicy, error::TenancyError, tenant_ref::TenantRef};
    use axum::{
        body::Body,
        http::{Method, Request, StatusCode},
    };
    use futures::future::BoxFuture;
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
    async fn owns_the_complete_context_registry_inventory() -> Result<(), Box<dyn std::error::Error>> {
        let routes = super::ContextRegistryRoutes::new(Arc::new(RejectingResolver)).into_router();
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
            let response = routes.clone().oneshot(request).await?;
            assert_ne!(response.status(), StatusCode::NOT_FOUND, "{method} {path} was not routed");
            assert_ne!(response.status(), StatusCode::METHOD_NOT_ALLOWED, "{method} {path} lost its method");
        }
        Ok(())
    }
}
