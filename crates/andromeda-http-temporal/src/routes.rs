//! Typed, state-complete inventory of temporal HTTP routes.

use crate::{
    operation_query,
    state::{TemporalRouteState, TemporalStateResolver},
    temporal,
};
use andromeda_extensions::enabled::EnabledExtensions;
use andromeda_http_extraction::scoped_response::echo_scope_headers;
use axum::{
    Router,
    middleware::from_fn,
    routing::{delete, get, patch, post},
};
use std::sync::Arc;

/// The state-complete temporal route fragment consumed by router assembly.
pub struct TemporalRoutes {
    /// Router whose focused temporal state has already been supplied.
    router: Router,
}

impl TemporalRoutes {
    /// Construct the complete temporal route inventory with only a focused
    /// tenant resolver and the enabled-extension set.
    ///
    /// `DELETE /temporal/entities` is added only when the
    /// `temporal_delete_by_query` extension is enabled; when it is not, the
    /// route is absent and the method answers `405`, exactly as a conformant
    /// broker that never implemented it does.
    pub fn new(resolver: Arc<dyn TemporalStateResolver>, enabled: EnabledExtensions) -> TemporalRoutes {
        let mut entity_routes = Router::new()
            .route("/", post(temporal::create_or_update::create_or_update))
            .route("/", get(temporal::query::query))
            .route("/{id}", get(temporal::retrieve::get))
            .route("/{id}", delete(temporal::delete::delete))
            .route("/{id}/attrs", post(temporal::add_attributes::add_attrs))
            .route("/{id}/attrs/{attr_id}", delete(temporal::delete_attribute::delete_attr))
            .route(
                "/{id}/attrs/{attr_id}/{instance_id}",
                patch(temporal::update_attribute_instance::update_attr_instance),
            )
            .route(
                "/{id}/attrs/{attr_id}/{instance_id}",
                delete(temporal::delete_attribute_instance::delete_attr_instance),
            );
        if enabled.temporal_delete_by_query_enabled() {
            entity_routes = entity_routes.route("/", delete(temporal::delete_by_query::delete_by_query));
        }
        let operation_routes = Router::new().route("/query", post(operation_query::query));
        let state = TemporalRouteState::new(resolver);
        let router = Router::new()
            .nest("/entities", entity_routes)
            .nest("/entityOperations", operation_routes)
            .layer(from_fn(echo_scope_headers))
            .with_state(state);
        TemporalRoutes { router }
    }

    /// Consume the typed fragment and return its state-complete Axum router for assembly.
    pub fn into_router(self) -> Router {
        self.router
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        routes::TemporalRoutes,
        state::{TemporalRouteServices, TemporalStateResolver},
    };
    use andromeda_extensions::enabled::EnabledExtensions;
    use andromeda_tenancy::{creation_policy::TenantCreationPolicy, error::TenancyError, tenant_ref::TenantRef};
    use axum::{
        body::Body,
        http,
        http::{Method, Request, StatusCode},
    };
    use futures::future::BoxFuture;
    use std::{error::Error, sync::Arc};
    use tower::ServiceExt;

    struct RejectingResolver;

    impl TemporalStateResolver for RejectingResolver {
        fn resolve<'a>(
            &'a self,
            _tenant: &'a TenantRef,
            _creation_policy: TenantCreationPolicy,
        ) -> BoxFuture<'a, Result<Arc<TemporalRouteServices>, TenancyError>> {
            Box::pin(async { Err(TenancyError::MultiTenancyDisabled) })
        }
    }

    fn request(method: Method, path: &str) -> Result<Request<Body>, http::Error> {
        Request::builder()
            .method(method)
            .uri(path)
            .header("content-type", "application/json")
            .body(Body::from("{}"))
    }

    #[tokio::test]
    async fn complete_temporal_route_inventory_is_owned_by_this_fragment() -> Result<(), Box<dyn Error>> {
        let routes = TemporalRoutes::new(Arc::new(RejectingResolver), EnabledExtensions::none()).into_router();
        for (method, path) in [
            (Method::POST, "/entities"),
            (Method::GET, "/entities"),
            (Method::GET, "/entities/urn:ngsi-ld:Entity:1"),
            (Method::DELETE, "/entities/urn:ngsi-ld:Entity:1"),
            (Method::POST, "/entities/urn:ngsi-ld:Entity:1/attrs"),
            (Method::DELETE, "/entities/urn:ngsi-ld:Entity:1/attrs/speed"),
            (Method::PATCH, "/entities/urn:ngsi-ld:Entity:1/attrs/speed/urn:ngsi-ld:Instance:1"),
            (Method::DELETE, "/entities/urn:ngsi-ld:Entity:1/attrs/speed/urn:ngsi-ld:Instance:1"),
            (Method::POST, "/entityOperations/query"),
        ] {
            let response = routes.clone().oneshot(request(method.clone(), path)?).await?;
            assert_ne!(response.status(), StatusCode::NOT_FOUND, "{method} {path} was not routed");
            assert_ne!(response.status(), StatusCode::METHOD_NOT_ALLOWED, "{method} {path} lost its allowed method");
        }
        Ok(())
    }

    #[tokio::test]
    async fn methods_outside_the_inventory_are_rejected() -> Result<(), Box<dyn Error>> {
        let routes = TemporalRoutes::new(Arc::new(RejectingResolver), EnabledExtensions::none()).into_router();
        for (method, path) in [
            (Method::PUT, "/entities"),
            (Method::POST, "/entities/urn:ngsi-ld:Entity:1"),
            (Method::GET, "/entityOperations/query"),
        ] {
            let response = routes.clone().oneshot(request(method, path)?).await?;
            pretty_assertions::assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
        }
        Ok(())
    }

    /// With the extension off, `DELETE /temporal/entities` is not routed and the
    /// method is refused, exactly as a conformant broker that never implemented
    /// it answers.
    #[tokio::test]
    async fn delete_by_query_is_absent_when_the_extension_is_off() -> Result<(), Box<dyn Error>> {
        let routes = TemporalRoutes::new(Arc::new(RejectingResolver), EnabledExtensions::none()).into_router();
        let response = routes.oneshot(request(Method::DELETE, "/entities")?).await?;
        pretty_assertions::assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
        Ok(())
    }

    /// With the extension on, `DELETE /temporal/entities` is routed: it reaches
    /// the handler rather than answering `404`/`405`.
    #[tokio::test]
    async fn delete_by_query_is_routed_when_the_extension_is_on() -> Result<(), Box<dyn Error>> {
        let routes = TemporalRoutes::new(Arc::new(RejectingResolver), EnabledExtensions::all()).into_router();
        let response = routes.oneshot(request(Method::DELETE, "/entities?type=Vehicle&drop=speed")?).await?;
        assert_ne!(response.status(), StatusCode::NOT_FOUND, "DELETE /temporal/entities was not routed");
        assert_ne!(response.status(), StatusCode::METHOD_NOT_ALLOWED, "DELETE /temporal/entities lost its method");
        Ok(())
    }
}
