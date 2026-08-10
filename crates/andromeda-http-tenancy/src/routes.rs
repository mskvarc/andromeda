//! Typed, state-complete tenant-administration route inventory.

use crate::{handler, state::TenancyRouteState};
use andromeda_tenancy::administration::{OrphanTenantEvictor, TenantDirectory};
use axum::{
    Router,
    routing::{get, post},
};
use std::sync::Arc;

/// The complete tenant-administration route fragment.
pub struct TenancyRoutes {
    /// Router whose focused state has already been supplied.
    router: Router,
}

impl TenancyRoutes {
    /// Construct the tenant query and orphan-eviction routes.
    pub fn new(directory: Arc<dyn TenantDirectory>, evictor: Arc<dyn OrphanTenantEvictor>) -> TenancyRoutes {
        let router = Router::new()
            .route("/", get(handler::query))
            .route("/evictOrphans", post(handler::evict_orphans))
            .with_state(TenancyRouteState::new(directory, evictor));
        TenancyRoutes {
            router: Router::new().nest("/tenants", router),
        }
    }

    /// Consume the state-complete route fragment.
    pub fn into_router(self) -> Router {
        self.router
    }
}

#[cfg(test)]
mod tests {
    use crate::routes::TenancyRoutes;
    use andromeda_ngsi_ld_lexicon::tenant::TenantName;
    use andromeda_tenancy::{
        administration::{OrphanTenantEvictor, TenantDirectory},
        error::TenancyError,
    };
    use async_trait::async_trait;
    use axum::{
        body::Body,
        http::{Method, Request, StatusCode},
    };
    use std::sync::Arc;
    use tower::ServiceExt;

    struct TenantAdministration;

    #[async_trait]
    impl TenantDirectory for TenantAdministration {
        async fn registered_tenants(&self) -> Result<Vec<TenantName>, TenancyError> {
            Ok(Vec::new())
        }
    }

    #[async_trait]
    impl OrphanTenantEvictor for TenantAdministration {
        async fn evict_orphan_runtimes(&self) -> usize {
            0
        }
    }

    #[tokio::test]
    async fn tenancy_fragment_owns_query_and_eviction_paths() -> Result<(), Box<dyn std::error::Error>> {
        let administration = Arc::new(TenantAdministration);
        let directory: Arc<dyn TenantDirectory> = administration.clone();
        let evictor: Arc<dyn OrphanTenantEvictor> = administration;
        let routes = TenancyRoutes::new(directory, evictor).into_router();
        for (method, path) in [(Method::GET, "/tenants"), (Method::POST, "/tenants/evictOrphans")] {
            let response = routes.clone().oneshot(Request::builder().method(method).uri(path).body(Body::empty())?).await?;
            pretty_assertions::assert_eq!(response.status(), StatusCode::OK);
        }
        Ok(())
    }
}
