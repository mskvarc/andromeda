//! Typed, state-complete inventory of snapshot routes.

use crate::{
    handler,
    state::{SnapshotRouteState, SnapshotStateResolver},
};
use axum::{
    Router,
    routing::{get, post},
};
use std::sync::Arc;

/// The complete snapshot route fragment.
pub struct SnapshotRoutes {
    /// The `/snapshots` routes with the tenant resolver bound as state.
    router: Router,
}

impl SnapshotRoutes {
    /// Construct every snapshot route with only its focused resolver.
    pub fn new(resolver: Arc<dyn SnapshotStateResolver>) -> SnapshotRoutes {
        let snapshot_routes = Router::new()
            .route("/", post(handler::create).delete(handler::purge))
            .route("/{snapshot_id}", get(handler::get).patch(handler::update).delete(handler::delete))
            .route("/{snapshot_id}/clone", post(handler::clone));
        let router = Router::new().nest("/snapshots", snapshot_routes).with_state(SnapshotRouteState::new(resolver));
        SnapshotRoutes { router }
    }

    /// Consume the state-complete fragment.
    pub fn into_router(self) -> Router {
        self.router
    }
}

#[cfg(test)]
mod tests {
    use crate::state::{SnapshotRouteServices, SnapshotStateResolver};
    use andromeda_tenancy::{creation_policy::TenantCreationPolicy, error::TenancyError, tenant_ref::TenantRef};
    use axum::{
        body::Body,
        http::{Method, Request, StatusCode},
    };
    use futures::future::BoxFuture;
    use pretty_assertions::assert_ne;
    use std::{error::Error, sync::Arc};
    use tower::ServiceExt;

    struct RejectingResolver;

    impl SnapshotStateResolver for RejectingResolver {
        fn resolve<'a>(
            &'a self,
            _tenant: &'a TenantRef,
            _creation_policy: TenantCreationPolicy,
        ) -> BoxFuture<'a, Result<Arc<SnapshotRouteServices>, TenancyError>> {
            Box::pin(async { Err(TenancyError::MultiTenancyDisabled) })
        }
    }

    #[tokio::test]
    async fn owns_the_complete_snapshot_inventory() -> Result<(), Box<dyn Error>> {
        let routes = super::SnapshotRoutes::new(Arc::new(RejectingResolver)).into_router();
        for (method, path) in [
            (Method::POST, "/snapshots"),
            (Method::DELETE, "/snapshots"),
            (Method::GET, "/snapshots/urn:ngsi-ld:Snapshot:1"),
            (Method::PATCH, "/snapshots/urn:ngsi-ld:Snapshot:1"),
            (Method::DELETE, "/snapshots/urn:ngsi-ld:Snapshot:1"),
            (Method::POST, "/snapshots/urn:ngsi-ld:Snapshot:1/clone"),
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
