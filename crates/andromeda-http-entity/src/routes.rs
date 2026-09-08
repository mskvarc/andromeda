//! Typed, state-complete inventory of entity HTTP routes.

use crate::{
    attribute,
    entity,
    entity_operations,
    entity_type,
    state::{EntityRouteState, EntityStateResolver},
};
use andromeda_http_extraction::scoped_response::echo_scope_headers;
use axum::{
    Router,
    middleware::from_fn,
    routing::{delete, get, patch, post, put},
};
use std::sync::Arc;

/// The state-complete entity route fragment consumed by current router assembly.
pub struct EntityRoutes {
    /// Router whose focused entity state has already been supplied.
    router: Router,
}

impl EntityRoutes {
    /// Construct every entity, entity-operation, type, and attribute route with only a focused tenant resolver.
    pub fn new(resolver: Arc<dyn EntityStateResolver>) -> EntityRoutes {
        let entity_routes = Router::new()
            .route("/", post(entity::create::create))
            .route("/", get(entity::query::query))
            .route("/", delete(entity::delete_by_query::delete_by_query))
            .route("/", patch(entity::missing_identifier::patch_no_id))
            .route("/", put(entity::missing_identifier::put_no_id))
            .route("/{id}", get(entity::retrieve::get))
            .route("/{id}", delete(entity::delete::delete))
            .route("/{id}", patch(entity::merge::merge))
            .route("/{id}", put(entity::replace::update))
            .route("/{id}/attrs", post(entity::append_attributes::append_attrs))
            .route("/{id}/attrs", patch(entity::update_attributes::update_attrs))
            .route("/{id}/attrs/{attr_id}", patch(entity::partial_update_attribute::partial_update_attr))
            .route("/{id}/attrs/{attr_id}", delete(entity::delete_attribute::delete_attr))
            .route("/{id}/attrs/{attr_id}", put(entity::replace_attribute::replace_attr));

        // The batch query is consumption over POST, so it is added after the
        // guard layer and stays outside it.
        let entity_operation_routes = Router::new()
            .route("/create", post(entity_operations::create::batch_create))
            .route("/upsert", post(entity_operations::upsert::batch_upsert))
            .route("/update", post(entity_operations::update::batch_update))
            .route("/merge", post(entity_operations::merge::batch_merge))
            .route("/delete", post(entity_operations::delete::batch_delete))
            .route("/query", post(entity_operations::query::query));

        let type_routes = Router::new().route("/", get(entity_type::query)).route("/{type_}", get(entity_type::get));
        let attribute_routes = Router::new().route("/", get(attribute::query)).route("/{attr_id}", get(attribute::get));
        let state = EntityRouteState::new(resolver);
        let router = Router::new()
            .nest("/entities", entity_routes)
            .nest("/entityOperations", entity_operation_routes)
            .nest("/types", type_routes)
            .nest("/attributes", attribute_routes)
            .layer(from_fn(echo_scope_headers))
            .with_state(state);
        EntityRoutes { router }
    }

    /// Consume the typed fragment and return its state-complete Axum router for assembly.
    pub fn into_router(self) -> Router {
        self.router
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        routes::EntityRoutes,
        state::{EntityRouteServices, EntityStateResolver},
    };
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

    impl EntityStateResolver for RejectingResolver {
        fn resolve<'a>(
            &'a self,
            _tenant: &'a TenantRef,
            _creation_policy: TenantCreationPolicy,
        ) -> BoxFuture<'a, Result<Arc<EntityRouteServices>, TenancyError>> {
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
    async fn complete_entity_route_inventory_is_owned_by_this_fragment() -> Result<(), Box<dyn Error>> {
        let routes = EntityRoutes::new(Arc::new(RejectingResolver)).into_router();
        for (method, path) in [
            (Method::POST, "/entities"),
            (Method::GET, "/entities"),
            (Method::DELETE, "/entities"),
            (Method::PATCH, "/entities"),
            (Method::PUT, "/entities"),
            (Method::GET, "/entities/urn:ngsi-ld:Entity:1"),
            (Method::DELETE, "/entities/urn:ngsi-ld:Entity:1"),
            (Method::PATCH, "/entities/urn:ngsi-ld:Entity:1"),
            (Method::PUT, "/entities/urn:ngsi-ld:Entity:1"),
            (Method::POST, "/entities/urn:ngsi-ld:Entity:1/attrs"),
            (Method::PATCH, "/entities/urn:ngsi-ld:Entity:1/attrs"),
            (Method::PATCH, "/entities/urn:ngsi-ld:Entity:1/attrs/speed"),
            (Method::DELETE, "/entities/urn:ngsi-ld:Entity:1/attrs/speed"),
            (Method::PUT, "/entities/urn:ngsi-ld:Entity:1/attrs/speed"),
            (Method::POST, "/entityOperations/create"),
            (Method::POST, "/entityOperations/upsert"),
            (Method::POST, "/entityOperations/update"),
            (Method::POST, "/entityOperations/merge"),
            (Method::POST, "/entityOperations/delete"),
            (Method::POST, "/entityOperations/query"),
            (Method::GET, "/types"),
            (Method::GET, "/types/Vehicle"),
            (Method::GET, "/attributes"),
            (Method::GET, "/attributes/speed"),
        ] {
            let response = routes.clone().oneshot(request(method.clone(), path)?).await?;
            assert_ne!(response.status(), StatusCode::NOT_FOUND, "{method} {path} was not routed");
            assert_ne!(response.status(), StatusCode::METHOD_NOT_ALLOWED, "{method} {path} lost its allowed method");
        }
        Ok(())
    }

    /// Snapshot-scoped Core operations, including writes, reach the ordinary
    /// handlers with the branch target resolved at the HTTP boundary.
    #[tokio::test]
    async fn snapshot_scoped_core_operations_reach_their_handlers() -> Result<(), Box<dyn Error>> {
        let routes = EntityRoutes::new(Arc::new(RejectingResolver)).into_router();
        for (method, path) in [
            (Method::POST, "/entities"),
            (Method::PATCH, "/entities/urn:ngsi-ld:Entity:1"),
            (Method::DELETE, "/entities/urn:ngsi-ld:Entity:1/attrs/speed"),
            (Method::POST, "/entityOperations/upsert"),
            (Method::GET, "/entities"),
            (Method::POST, "/entityOperations/query"),
            (Method::GET, "/types"),
        ] {
            let mut scoped = request(method.clone(), path)?;
            scoped
                .headers_mut()
                .insert("ngsild-snapshot", http::HeaderValue::from_static("urn:ngsi-ld:Snapshot:1"));
            let response = routes.clone().oneshot(scoped).await?;
            assert_ne!(response.status(), StatusCode::UNPROCESSABLE_ENTITY, "{method} {path} must reach its handler");
        }
        Ok(())
    }

    #[tokio::test]
    async fn methods_outside_the_inventory_are_rejected() -> Result<(), Box<dyn Error>> {
        let routes = EntityRoutes::new(Arc::new(RejectingResolver)).into_router();
        for (method, path) in [
            (Method::POST, "/types"),
            (Method::DELETE, "/attributes/speed"),
            (Method::GET, "/entityOperations/create"),
        ] {
            let response = routes.clone().oneshot(request(method, path)?).await?;
            pretty_assertions::assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
        }
        Ok(())
    }
}
