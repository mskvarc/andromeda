//! Typed, state-complete tenant-management route inventory.

use crate::{handler, state::TenancyRouteState};
use andromeda_tenancy::{administration::TenantAdministration, usage_probe::TenantUsageProbe};
use axum::{Router, routing::get};
use std::sync::Arc;

/// Whether the tenant-management surface is routed.
///
/// A two-variant switch the composition root derives from the tenant-management
/// extension flag. It exists so this crate gates its routes without depending on
/// the extension registry, which sits above it in the dependency graph (the HTTP
/// stack depends on this crate for tenant-header propagation, so the reverse edge
/// would be a cycle).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TenantManagement {
    /// The `/tenants` introspection and lifecycle routes are served.
    Enabled,
    /// The surface is absent; every method on `/tenants` answers `404`.
    Disabled,
}

/// The complete tenant-management route fragment.
pub struct TenancyRoutes {
    /// Router whose focused state has already been supplied.
    router: Router,
}

impl TenancyRoutes {
    /// Construct the tenant-management routes, present only when tenant
    /// management is [`TenantManagement::Enabled`].
    ///
    /// When it is off the nest carries no routes, so every method on `/tenants`
    /// answers `404` exactly like a broker that never implemented the surface. No
    /// route drops a tenant's physical database under any setting.
    pub fn new(administration: Arc<dyn TenantAdministration>, usage: Arc<dyn TenantUsageProbe>, management: TenantManagement) -> TenancyRoutes {
        let mut router = Router::new();
        if matches!(management, TenantManagement::Enabled) {
            router = router
                .route("/", get(handler::collection::query))
                .route("/{tenantName}", get(handler::introspection::get).patch(handler::lifecycle::patch));
        }
        let router = router.with_state(TenancyRouteState::new(administration, usage));
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
    use crate::routes::{TenancyRoutes, TenantManagement};
    use andromeda_ngsi_ld_lexicon::tenant::TenantName;
    use andromeda_tenancy::{
        administration::TenantAdministration,
        database_name::TenantDatabaseName,
        error::TenancyError,
        residency::TenantResidency,
        status::TenantStatus,
        tenant_keyset::TenantKeyset,
        tenant_listing::TenantListing,
        tenant_record::TenantRecord,
        tenant_usage::TenantUsage,
        tenant_view::TenantView,
        usage_probe::TenantUsageProbe,
    };
    use async_trait::async_trait;
    use axum::{
        Router,
        body::Body,
        http::{Method, Request, StatusCode},
    };
    use chrono::{DateTime, Utc};
    use std::{error::Error, sync::Arc};
    use tower::ServiceExt;

    fn epoch() -> DateTime<Utc> {
        DateTime::from_timestamp(0, 0).expect("the Unix epoch is representable")
    }

    fn view(name: &str) -> TenantView {
        let name = TenantName::try_from(name.to_string()).expect("a valid tenant name");
        let record = TenantRecord::new(
            name.clone(),
            TenantStatus::Active,
            TenantDatabaseName::from(format!("db_{}", name.as_str())),
            epoch(),
            epoch(),
        );
        TenantView::new(record, TenantResidency::NotResident)
    }

    struct FakeAdministration;

    #[async_trait]
    impl TenantAdministration for FakeAdministration {
        async fn tenant(&self, name: &TenantName) -> Result<Option<TenantView>, TenancyError> {
            Ok(Some(view(name.as_str())))
        }

        async fn list_tenants(&self, _keyset: &TenantKeyset, _limit: i64) -> Result<TenantListing, TenancyError> {
            Ok(TenantListing::new(vec![view("acme")], 1))
        }

        async fn set_admin_status(&self, name: &TenantName, _status: TenantStatus) -> Result<Option<TenantView>, TenancyError> {
            Ok(Some(view(name.as_str())))
        }
    }

    struct FakeUsage;

    #[async_trait]
    impl TenantUsageProbe for FakeUsage {
        async fn usage(&self, _database: &TenantDatabaseName) -> Result<TenantUsage, TenancyError> {
            Ok(TenantUsage {
                pool_connections: 1,
                database_size: 2,
                entity_count: 3,
                snapshot_count: 0,
                snapshot_quota: None,
            })
        }
    }

    fn routes(management: TenantManagement) -> Router {
        let administration: Arc<dyn TenantAdministration> = Arc::new(FakeAdministration);
        let usage: Arc<dyn TenantUsageProbe> = Arc::new(FakeUsage);
        TenancyRoutes::new(administration, usage, management).into_router()
    }

    #[tokio::test]
    async fn the_management_routes_are_present_when_the_extension_is_on() -> Result<(), Box<dyn Error>> {
        let router = routes(TenantManagement::Enabled);
        for (method, path, expected) in [
            (Method::GET, "/tenants", StatusCode::OK),
            (Method::GET, "/tenants/acme", StatusCode::OK),
            (Method::PATCH, "/tenants/acme", StatusCode::NO_CONTENT),
        ] {
            // Clause 6.3.4: a PATCH declares its payload length.
            let payload = if method == Method::PATCH { "{\"isActive\":false}" } else { "" };
            let request = Request::builder()
                .method(method.clone())
                .uri(path)
                .header("content-type", "application/json")
                .header("content-length", payload.len())
                .body(Body::from(payload))?;
            let response = router.clone().oneshot(request).await?;
            pretty_assertions::assert_eq!(response.status(), expected, "{method} {path}");
        }
        Ok(())
    }

    #[tokio::test]
    async fn the_management_routes_are_absent_when_the_extension_is_off() -> Result<(), Box<dyn Error>> {
        let router = routes(TenantManagement::Disabled);
        for (method, path) in [(Method::GET, "/tenants"), (Method::GET, "/tenants/acme"), (Method::PATCH, "/tenants/acme")] {
            let request = Request::builder().method(method.clone()).uri(path).body(Body::empty())?;
            let response = router.clone().oneshot(request).await?;
            pretty_assertions::assert_eq!(response.status(), StatusCode::NOT_FOUND, "{method} {path}");
        }
        Ok(())
    }
}
