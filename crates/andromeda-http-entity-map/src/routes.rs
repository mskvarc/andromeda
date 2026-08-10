//! Typed, state-complete inventory of Entity Map routes.

use crate::{
    handler,
    state::{EntityMapRouteState, EntityMapStateResolver},
};
use axum::{Router, routing::get};
use std::sync::Arc;

/// Current and temporal Entity Map route fragments.
pub struct EntityMapRoutes {
    router: Router,
}

impl EntityMapRoutes {
    /// Construct all Entity Map routes with focused state.
    pub fn new(resolver: Arc<dyn EntityMapStateResolver>) -> EntityMapRoutes {
        let state = EntityMapRouteState::new(resolver);
        let current_routes = Router::new()
            .route("/", get(handler::create_from_query).post(handler::create_from_query_post))
            .route("/{entity_map_id}", get(handler::get).patch(handler::update).delete(handler::delete));
        let temporal_routes = Router::new().route("/", get(handler::create_temporal_from_query).post(handler::create_temporal_from_query_post));
        let router = Router::new()
            .nest("/entityMaps", current_routes)
            .nest("/temporal/entityMaps", temporal_routes)
            .with_state(state);
        EntityMapRoutes { router }
    }

    /// Consume both state-complete route fragments.
    pub fn into_router(self) -> Router {
        self.router
    }
}

#[cfg(test)]
mod tests {
    use crate::state::{EntityMapRouteServices, EntityMapStateResolver};
    use andromeda_tenancy::{creation_policy::TenantCreationPolicy, error::TenancyError, tenant_ref::TenantRef};
    use axum::{
        body::Body,
        http::{Method, Request, StatusCode},
    };
    use futures::future::BoxFuture;
    use std::sync::Arc;
    use tower::ServiceExt;

    struct RejectingResolver;

    impl EntityMapStateResolver for RejectingResolver {
        fn resolve<'a>(
            &'a self,
            _tenant: &'a TenantRef,
            _creation_policy: TenantCreationPolicy,
        ) -> BoxFuture<'a, Result<Arc<EntityMapRouteServices>, TenancyError>> {
            Box::pin(async { Err(TenancyError::MultiTenancyDisabled) })
        }
    }

    #[tokio::test]
    async fn owns_current_and_temporal_entity_map_inventory() -> Result<(), Box<dyn std::error::Error>> {
        let routes = super::EntityMapRoutes::new(Arc::new(RejectingResolver)).into_router();
        for (method, path) in [
            (Method::GET, "/entityMaps"),
            (Method::POST, "/entityMaps"),
            (Method::GET, "/entityMaps/urn:ngsi-ld:EntityMap:1"),
            (Method::PATCH, "/entityMaps/urn:ngsi-ld:EntityMap:1"),
            (Method::DELETE, "/entityMaps/urn:ngsi-ld:EntityMap:1"),
            (Method::GET, "/temporal/entityMaps"),
            (Method::POST, "/temporal/entityMaps"),
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
