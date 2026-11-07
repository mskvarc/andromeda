//! Assembly of named state-complete fragments under the public API root.

use crate::{management::ManagementRoutes, routes::ApiRoutes};
use andromeda_http_extraction::{api_root::ConfiguredApiRoot, operation_hit_scope::scope_operation_hits, request_links::validate_request_links};
use andromeda_http_response::method_not_allowed::registered_on;
use andromeda_http_stack::stack::HttpStack;
use andromeda_ngsi_ld_request::pagination::policy::PaginationPolicy;
use axum::{Extension, Router, middleware::from_fn};
use tower_http::normalize_path::NormalizePath;

/// Path prefix every NGSI-LD route is served under.
pub const API_ROOT_PATH: &str = "/ngsi-ld/v1";

/// Fully assembled, state-complete public HTTP API.
///
/// The NGSI-LD families live under `/ngsi-ld/v1`; metrics are served on the
/// management plane, not the data plane. In merged mode the management probes
/// (`/livez`, `/readyz`) and the optional scrape (`/metrics`) sit at the root as
/// siblings of the `/ngsi-ld/v1` nest, behind the full process stack -- the
/// documented cost of collapsing both planes onto one port.
pub struct HttpApi {
    /// Process-stacked service with path normalization outside Axum routing.
    service: NormalizePath<Router>,
}

impl HttpApi {
    /// Merge every required route family under `/ngsi-ld/v1` and apply the configured stack.
    ///
    /// When `merged_management` is `Some`, the management endpoints are mounted at
    /// the root of the assembled service, sharing the data-plane listener and its
    /// full middleware stack; when `None`, only the NGSI-LD data plane is served.
    #[must_use]
    pub fn assemble(routes: ApiRoutes, merged_management: Option<ManagementRoutes>, stack: &HttpStack) -> HttpApi {
        let ngsi = Router::new()
            .merge(routes.entity.into_router())
            .nest("/temporal", routes.temporal.into_router())
            .merge(routes.context_registry.into_router())
            .merge(routes.registration.into_router())
            .merge(routes.subscription.into_router())
            .merge(routes.entity_map.into_router())
            .merge(routes.snapshot.into_router())
            .merge(routes.source_identity.into_router())
            .merge(routes.notification_receiver.into_router())
            .merge(routes.tenancy.into_router())
            .merge(routes.health.into_router())
            .merge(routes.component_report.into_router());
        let ngsi = registered_on(ngsi);
        let mut api_root = Router::new().nest(
            API_ROOT_PATH,
            stack.apply_api(ngsi_request_layers(ngsi, routes.configured_api_root, routes.pagination)),
        );
        // Merged mode mounts the management endpoints as root siblings of the
        // NGSI-LD nest, outside the operation-hit scope, so a metrics scrape is
        // never counted as an NGSI-LD operation.
        if let Some(management) = merged_management {
            api_root = api_root.merge(management.into_root_router());
        }
        HttpApi {
            service: stack.apply_process(api_root),
        }
    }

    /// Consume the API and return the generic state-complete HTTP service.
    #[must_use]
    pub fn into_service(self) -> NormalizePath<Router> {
        self.service
    }

    /// Return a bare router serving the same state-complete API, for a transport
    /// that drives an [`axum::Router`] directly rather than a tower service.
    ///
    /// axum's HTTP/3 adapter accepts a [`Router`], not the process-stacked
    /// [`NormalizePath<Router>`] the TCP listener serves, because trailing-slash
    /// normalization is applied outside axum routing. Wrapping the normalized
    /// service as the fallback of an otherwise empty router preserves that step:
    /// every request misses the empty router's (absent) routes and is handled by
    /// the normalized inner service, so HTTP/3 trims trailing slashes exactly as
    /// the TCP path does.
    pub fn h3_router(&self) -> Router {
        // A shallow, Arc-backed clone of the shared route table, not a deep copy
        // of the assembled API; the TCP listener still consumes the original.
        Router::new().fallback_service(self.service.clone())
    }
}

/// The layers every NGSI-LD request passes before any route.
fn ngsi_request_layers(ngsi: Router, configured_api_root: Option<ConfiguredApiRoot>, pagination: PaginationPolicy) -> Router {
    // Clause 5.13.3.5: one NGSI-LD request resolves the same `@context`
    // several times internally (write screening, then the write itself);
    // an operation scope makes those internal resolutions count as one use.
    let ngsi = ngsi.layer(from_fn(scope_operation_hits));
    // Clauses 5.5.4 and 6.3.5: every NGSI-LD request's `Link` headers are
    // read once, before any route, so a malformed `Link` or several
    // context links are refused for every verb and no extractor parses
    // the header again.
    let ngsi = ngsi.layer(from_fn(validate_request_links));
    // Clauses 5.5.9.1 and 5.5.9.2: every paged collection pages by the one
    // policy the operator configured.
    let ngsi = ngsi.layer(Extension(pagination));
    // Clause 6.2: a configured `apiRoot`, prefix included, is the one every
    // route builds on.
    match configured_api_root {
        Some(configured) => ngsi.layer(Extension(configured)),
        None => ngsi,
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        api::{HttpApi, ngsi_request_layers},
        management::ManagementRoutes,
        routes::ApiRoutes,
    };
    use andromeda_component_report::{ComponentReport, ComponentVersion, ConnectionCapacity};
    use andromeda_configuration::schema::{
        feature_mode::FeatureMode,
        http::{
            HttpStackConfig,
            cors::{CorsConfig, CorsMode},
            csrf::CsrfConfig,
            security_headers::SecurityHeadersConfig,
            server_timing::ServerTimingConfig,
        },
        request_limits::RequestLimitsConfig,
    };
    use andromeda_extensions::enabled::EnabledExtensions;
    use andromeda_health::aggregate::Health;
    use andromeda_http_component_report::routes::ComponentReportRoutes;
    use andromeda_http_context_registry::{
        routes::ContextRegistryRoutes,
        state::{ContextRegistryRouteServices, ContextRegistryStateResolver},
    };
    use andromeda_http_entity::{
        routes::EntityRoutes,
        state::{EntityRouteServices, EntityStateResolver},
    };
    use andromeda_http_entity_map::{
        routes::EntityMapRoutes,
        state::{EntityMapRouteServices, EntityMapStateResolver},
    };
    use andromeda_http_extraction::{
        accept_language::AcceptLanguageReading,
        api_root::{ApiRoot, ConfiguredApiRoot},
        pagination_policy::RequestPagination,
    };
    use andromeda_http_health::routes::HealthRoutes;
    use andromeda_http_notification_receiver::{
        routes::NotificationReceiverRoutes,
        state::{LeasedForwarding, NotificationReceiverResolver, ReceiverResolutionError},
    };
    use andromeda_http_registration::{
        routes::RegistrationRoutes,
        state::{RegistrationRouteServices, RegistrationStateResolver},
    };
    use andromeda_http_snapshot::{
        routes::SnapshotRoutes,
        state::{SnapshotRouteServices, SnapshotStateResolver},
    };
    use andromeda_http_source_identity::{
        default_tenant_contexts::{DefaultTenantContexts, DefaultTenantContextsResolver},
        routes::SourceIdentityRoutes,
        state::SourceIdentityState,
    };
    use andromeda_http_stack::stack::HttpStack;
    use andromeda_http_subscription::{
        routes::SubscriptionRoutes,
        state::{SubscriptionRouteServices, SubscriptionStateResolver},
    };
    use andromeda_http_telemetry::routes::TelemetryRoutes;
    use andromeda_http_temporal::{
        routes::TemporalRoutes,
        state::{TemporalRouteServices, TemporalStateResolver},
    };
    use andromeda_http_tenancy::routes::{TenancyRoutes, TenantManagement};
    use andromeda_ngsi_ld_lexicon::{
        context_source_pseudonym::ContextSourcePseudonym,
        identifier::Identifier,
        tenant::TenantName,
        tenant_database_name::TenantDatabaseName,
    };
    use andromeda_ngsi_ld_request::pagination::policy::PaginationPolicy;
    use andromeda_telemetry::recorder::TelemetryRecorder;
    use andromeda_tenancy::{
        administration::TenantAdministration,
        context_source_alias::ContextSourceAliases,
        creation_policy::TenantCreationPolicy,
        error::TenancyError,
        residency::TenantResidency,
        status::TenantStatus,
        tenant_listing::TenantListing,
        tenant_page_start::TenantPageStart,
        tenant_record::TenantRecord,
        tenant_ref::TenantRef,
        tenant_usage::TenantUsage,
        tenant_view::TenantView,
        usage_probe::TenantUsageProbe,
    };
    use async_trait::async_trait;
    use axum::{
        Router,
        body::{Body, to_bytes},
        http,
        http::{Method, Request, StatusCode},
        response::Response,
        routing::get,
    };
    use chrono::Utc;
    use futures::future::BoxFuture;
    use serde_json::Value;
    use std::{error::Error, sync::Arc};
    use tower::ServiceExt;
    use url::Url;

    macro_rules! rejecting_resolver {
        ($name:ident, $resolver:ident, $services:ident) => {
            struct $name;

            impl $resolver for $name {
                fn resolve<'a>(
                    &'a self,
                    _tenant: &'a TenantRef,
                    _creation_policy: TenantCreationPolicy,
                ) -> BoxFuture<'a, Result<Arc<$services>, TenancyError>> {
                    Box::pin(async { Err(TenancyError::MultiTenancyDisabled) })
                }
            }
        };
    }

    rejecting_resolver!(RejectEntity, EntityStateResolver, EntityRouteServices);
    rejecting_resolver!(RejectTemporal, TemporalStateResolver, TemporalRouteServices);
    rejecting_resolver!(RejectContextRegistry, ContextRegistryStateResolver, ContextRegistryRouteServices);
    rejecting_resolver!(RejectRegistration, RegistrationStateResolver, RegistrationRouteServices);
    rejecting_resolver!(RejectSubscription, SubscriptionStateResolver, SubscriptionRouteServices);
    rejecting_resolver!(RejectEntityMap, EntityMapStateResolver, EntityMapRouteServices);
    rejecting_resolver!(RejectSnapshot, SnapshotStateResolver, SnapshotRouteServices);

    struct RejectDefaultTenant;

    impl DefaultTenantContextsResolver for RejectDefaultTenant {
        fn resolve(&self) -> BoxFuture<'_, Result<DefaultTenantContexts, TenancyError>> {
            Box::pin(async { Err(TenancyError::MultiTenancyDisabled) })
        }
    }

    struct RejectReceiver;

    impl NotificationReceiverResolver for RejectReceiver {
        fn resolve<'a>(&'a self, subscription: &'a Identifier) -> BoxFuture<'a, Result<LeasedForwarding, ReceiverResolutionError>> {
            Box::pin(async move {
                Err(ReceiverResolutionError::Unknown {
                    subscription: subscription.clone(), // clone: the error owns the identifier
                })
            })
        }
    }

    fn fake_tenant_view(name: &TenantName) -> TenantView {
        let record = TenantRecord::new(
            name.clone(), // clone: the record owns its name
            TenantStatus::Active,
            TenantDatabaseName::from(format!("db_{}", name.as_str())),
            Utc::now(),
            Utc::now(),
        );
        TenantView::new(record, TenantResidency::NotResident)
    }

    struct FakeTenantAdministration;

    #[async_trait]
    impl TenantAdministration for FakeTenantAdministration {
        async fn tenant(&self, name: &TenantName) -> Result<Option<TenantView>, TenancyError> {
            Ok(Some(fake_tenant_view(name)))
        }

        async fn list_tenants(&self, _start: &TenantPageStart, _limit: u32) -> Result<TenantListing, TenancyError> {
            Ok(TenantListing::new(Vec::new(), 0))
        }

        async fn set_admin_status(&self, name: &TenantName, _status: TenantStatus) -> Result<Option<TenantView>, TenancyError> {
            Ok(Some(fake_tenant_view(name)))
        }
    }

    struct FakeTenantUsage;

    #[async_trait]
    impl TenantUsageProbe for FakeTenantUsage {
        async fn usage(&self, _database: &TenantDatabaseName) -> Result<TenantUsage, TenancyError> {
            Ok(TenantUsage {
                pool_connections: 0,
                database_size: 0,
                entity_count: 0,
                snapshot_count: 0,
                snapshot_quota: None,
            })
        }
    }

    fn build_routes() -> Result<ApiRoutes, Box<dyn Error>> {
        let administration: Arc<dyn TenantAdministration> = Arc::new(FakeTenantAdministration);
        let usage: Arc<dyn TenantUsageProbe> = Arc::new(FakeTenantUsage);
        Ok(ApiRoutes::builder()
            .entity(EntityRoutes::new(
                Arc::new(RejectEntity),
                EnabledExtensions::none(),
                AcceptLanguageReading::Ignored,
            ))
            .temporal(TemporalRoutes::new(
                Arc::new(RejectTemporal),
                EnabledExtensions::none(),
                AcceptLanguageReading::Ignored,
            ))
            .context_registry(ContextRegistryRoutes::new(Arc::new(RejectContextRegistry)))
            .registration(RegistrationRoutes::new(Arc::new(RejectRegistration), AcceptLanguageReading::Ignored))
            .subscription(SubscriptionRoutes::new(Arc::new(RejectSubscription)))
            .entity_map(EntityMapRoutes::new(Arc::new(RejectEntityMap), EnabledExtensions::none()))
            .snapshot(SnapshotRoutes::new(Arc::new(RejectSnapshot)))
            .source_identity(SourceIdentityRoutes::new(SourceIdentityState::new(
                "urn:ngsi-ld:ContextSourceIdentity:test".parse::<Identifier>()?,
                ContextSourceAliases::new("test".parse::<ContextSourcePseudonym>()?),
                Utc::now(),
                None,
                EnabledExtensions::none(),
                Arc::new(RejectDefaultTenant),
            )?))
            .notification_receiver(NotificationReceiverRoutes::new(Arc::new(RejectReceiver)))
            .tenancy(TenancyRoutes::new(administration, usage, TenantManagement::Enabled))
            .health(HealthRoutes::new(Health::builder().build()))
            .component_report(ComponentReportRoutes::new(vec![ComponentReport::PostgreSql {
                version: ComponentVersion::from("16.2".to_owned()),
                connections: ConnectionCapacity::new(200, 3, 150),
            }]))
            .configured_api_root(None)
            .pagination(PaginationPolicy::CursorDefault)
            .build())
    }

    fn test_stack() -> Result<HttpStack, Box<dyn Error>> {
        // Every optional edge feature is switched off so the routing assertions
        // observe fragment dispatch, not middleware side effects.
        let http_config = HttpStackConfig {
            cors: CorsConfig {
                mode: CorsMode::Disabled,
                ..CorsConfig::default()
            },
            server_timing: ServerTimingConfig {
                mode: FeatureMode::Disabled,
                ..ServerTimingConfig::default()
            },
            security_headers: SecurityHeadersConfig {
                mode: FeatureMode::Disabled,
                ..SecurityHeadersConfig::default()
            },
            csrf: CsrfConfig {
                mode: FeatureMode::Disabled,
                trusted_origins: Vec::new(),
            },
            ..HttpStackConfig::default()
        };
        Ok(HttpStack::new(http_config, RequestLimitsConfig::default())?)
    }

    fn test_api() -> Result<HttpApi, Box<dyn Error>> {
        Ok(HttpApi::assemble(build_routes()?, None, &test_stack()?))
    }

    fn management_routes(metrics: Option<TelemetryRoutes>) -> ManagementRoutes {
        ManagementRoutes {
            health: HealthRoutes::new(Health::builder().build()),
            metrics,
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
    async fn every_canonical_method_and_path_reaches_exactly_one_required_fragment() -> Result<(), Box<dyn Error>> {
        let service = test_api()?.into_service();
        for (method, path) in [
            (Method::POST, "/ngsi-ld/v1/entities"),
            (Method::GET, "/ngsi-ld/v1/entities"),
            (Method::DELETE, "/ngsi-ld/v1/entities"),
            (Method::GET, "/ngsi-ld/v1/entities/urn:ngsi-ld:Entity:1"),
            (Method::DELETE, "/ngsi-ld/v1/entities/urn:ngsi-ld:Entity:1"),
            (Method::PATCH, "/ngsi-ld/v1/entities/urn:ngsi-ld:Entity:1"),
            (Method::PUT, "/ngsi-ld/v1/entities/urn:ngsi-ld:Entity:1"),
            (Method::POST, "/ngsi-ld/v1/entities/urn:ngsi-ld:Entity:1/attrs"),
            (Method::PATCH, "/ngsi-ld/v1/entities/urn:ngsi-ld:Entity:1/attrs"),
            (Method::PATCH, "/ngsi-ld/v1/entities/urn:ngsi-ld:Entity:1/attrs/speed"),
            (Method::DELETE, "/ngsi-ld/v1/entities/urn:ngsi-ld:Entity:1/attrs/speed"),
            (Method::PUT, "/ngsi-ld/v1/entities/urn:ngsi-ld:Entity:1/attrs/speed"),
            (Method::POST, "/ngsi-ld/v1/notifications"),
            (Method::POST, "/ngsi-ld/v1/entityOperations/create"),
            (Method::POST, "/ngsi-ld/v1/entityOperations/upsert"),
            (Method::POST, "/ngsi-ld/v1/entityOperations/update"),
            (Method::POST, "/ngsi-ld/v1/entityOperations/merge"),
            (Method::POST, "/ngsi-ld/v1/entityOperations/delete"),
            (Method::POST, "/ngsi-ld/v1/entityOperations/query"),
            (Method::GET, "/ngsi-ld/v1/types"),
            (Method::GET, "/ngsi-ld/v1/types/Vehicle"),
            (Method::GET, "/ngsi-ld/v1/attributes"),
            (Method::GET, "/ngsi-ld/v1/attributes/speed"),
            (Method::POST, "/ngsi-ld/v1/temporal/entities"),
            (Method::GET, "/ngsi-ld/v1/temporal/entities"),
            (Method::GET, "/ngsi-ld/v1/temporal/entities/urn:ngsi-ld:Entity:1"),
            (Method::DELETE, "/ngsi-ld/v1/temporal/entities/urn:ngsi-ld:Entity:1"),
            (Method::POST, "/ngsi-ld/v1/temporal/entities/urn:ngsi-ld:Entity:1/attrs"),
            (Method::DELETE, "/ngsi-ld/v1/temporal/entities/urn:ngsi-ld:Entity:1/attrs/speed"),
            (
                Method::PATCH,
                "/ngsi-ld/v1/temporal/entities/urn:ngsi-ld:Entity:1/attrs/speed/urn:ngsi-ld:Instance:1",
            ),
            (
                Method::DELETE,
                "/ngsi-ld/v1/temporal/entities/urn:ngsi-ld:Entity:1/attrs/speed/urn:ngsi-ld:Instance:1",
            ),
            (Method::POST, "/ngsi-ld/v1/temporal/entityOperations/query"),
            (Method::POST, "/ngsi-ld/v1/jsonldContexts"),
            (Method::GET, "/ngsi-ld/v1/jsonldContexts"),
            (Method::GET, "/ngsi-ld/v1/jsonldContexts/context-1"),
            (Method::DELETE, "/ngsi-ld/v1/jsonldContexts/context-1"),
            (Method::POST, "/ngsi-ld/v1/csourceRegistrations"),
            (Method::GET, "/ngsi-ld/v1/csourceRegistrations"),
            (Method::GET, "/ngsi-ld/v1/csourceRegistrations/urn:ngsi-ld:ContextSourceRegistration:1"),
            (Method::PATCH, "/ngsi-ld/v1/csourceRegistrations/urn:ngsi-ld:ContextSourceRegistration:1"),
            (Method::DELETE, "/ngsi-ld/v1/csourceRegistrations/urn:ngsi-ld:ContextSourceRegistration:1"),
            (Method::POST, "/ngsi-ld/v1/subscriptions"),
            (Method::GET, "/ngsi-ld/v1/subscriptions"),
            (Method::GET, "/ngsi-ld/v1/subscriptions/urn:ngsi-ld:Subscription:1"),
            (Method::PATCH, "/ngsi-ld/v1/subscriptions/urn:ngsi-ld:Subscription:1"),
            (Method::DELETE, "/ngsi-ld/v1/subscriptions/urn:ngsi-ld:Subscription:1"),
            (Method::POST, "/ngsi-ld/v1/csourceSubscriptions"),
            (Method::GET, "/ngsi-ld/v1/csourceSubscriptions"),
            (Method::GET, "/ngsi-ld/v1/csourceSubscriptions/urn:ngsi-ld:Subscription:2"),
            (Method::PATCH, "/ngsi-ld/v1/csourceSubscriptions/urn:ngsi-ld:Subscription:2"),
            (Method::DELETE, "/ngsi-ld/v1/csourceSubscriptions/urn:ngsi-ld:Subscription:2"),
            (Method::GET, "/ngsi-ld/v1/entityMaps"),
            (Method::POST, "/ngsi-ld/v1/entityMaps"),
            (Method::GET, "/ngsi-ld/v1/entityMaps/urn:ngsi-ld:EntityMap:1"),
            (Method::PATCH, "/ngsi-ld/v1/entityMaps/urn:ngsi-ld:EntityMap:1"),
            (Method::DELETE, "/ngsi-ld/v1/entityMaps/urn:ngsi-ld:EntityMap:1"),
            (Method::GET, "/ngsi-ld/v1/temporal/entityMaps"),
            (Method::POST, "/ngsi-ld/v1/temporal/entityMaps"),
            (Method::POST, "/ngsi-ld/v1/snapshots"),
            (Method::DELETE, "/ngsi-ld/v1/snapshots"),
            (Method::GET, "/ngsi-ld/v1/snapshots/urn:ngsi-ld:Snapshot:1"),
            (Method::PATCH, "/ngsi-ld/v1/snapshots/urn:ngsi-ld:Snapshot:1"),
            (Method::DELETE, "/ngsi-ld/v1/snapshots/urn:ngsi-ld:Snapshot:1"),
            (Method::POST, "/ngsi-ld/v1/snapshots/urn:ngsi-ld:Snapshot:1/clone"),
            (Method::GET, "/ngsi-ld/v1/info/sourceIdentity"),
            (Method::GET, "/ngsi-ld/v1/tenants"),
            (Method::GET, "/ngsi-ld/v1/tenants/acme"),
            (Method::PATCH, "/ngsi-ld/v1/tenants/acme"),
            (Method::GET, "/ngsi-ld/v1/health"),
            (Method::GET, "/ngsi-ld/v1/ready"),
            (Method::GET, "/ngsi-ld/v1/live"),
            (Method::GET, "/ngsi-ld/v1/version"),
        ] {
            let response = service.clone().oneshot(request(method.clone(), path)?).await?;
            assert_ne!(response.status(), StatusCode::NOT_FOUND, "{method} {path} was not routed");
            assert_ne!(response.status(), StatusCode::METHOD_NOT_ALLOWED, "{method} {path} lost its method");
        }
        Ok(())
    }

    /// There is no orphan-eviction endpoint: the janitor sweeps orphaned
    /// runtimes automatically. `POST /tenants/evictOrphans` falls under the
    /// `/tenants/{tenantName}` resource, which serves only `GET` and `PATCH`, so
    /// the `POST` is answered `405` rather than reaching any handler.
    #[tokio::test]
    async fn the_retired_evict_orphans_route_is_absent() -> Result<(), Box<dyn Error>> {
        let service = test_api()?.into_service();
        let response = service.oneshot(request(Method::POST, "/ngsi-ld/v1/tenants/evictOrphans")?).await?;
        pretty_assertions::assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
        Ok(())
    }

    /// A request whose method a resource does not serve is answered a problem
    /// document rather than a bare status: the document names the method it
    /// refused and the request target, and Axum still attaches the `Allow`
    /// header naming what the resource serves (RFC 9110, clause 15.5.6).
    #[tokio::test]
    async fn a_wrong_method_request_gets_a_problem_document_and_the_allow_header() -> Result<(), Box<dyn Error>> {
        let service = test_api()?.into_service();
        let response = service.oneshot(request(Method::POST, "/ngsi-ld/v1/tenants/acme")?).await?;
        pretty_assertions::assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
        let allow = response.headers().get("allow").and_then(|value| value.to_str().ok());
        assert!(
            allow.is_some_and(|allow| allow.contains("GET") && allow.contains("PATCH") && !allow.contains("POST")),
            "{allow:?}"
        );
        let body = to_bytes(response.into_body(), usize::MAX).await?;
        let document: Value = serde_json::from_slice(&body)?;
        pretty_assertions::assert_eq!(document["type"], "https://uri.etsi.org/ngsi-ld/errors/MethodNotAllowed");
        pretty_assertions::assert_eq!(document["title"], "Method not allowed");
        pretty_assertions::assert_eq!(document["status"], 405);
        assert!(
            document["detail"].as_str().is_some_and(|detail| detail.contains("POST")),
            "{}",
            document["detail"]
        );
        pretty_assertions::assert_eq!(document["instance"], "/ngsi-ld/v1/tenants/acme");
        Ok(())
    }

    /// Clauses 6.4.3 and 6.3.2: the Entities collection serves `POST`, `GET`
    /// and `DELETE` only, so a `PUT` or `PATCH` on it is a wrong verb over the
    /// resource, with or without a trailing slash; Replace and Merge Entity are
    /// bound to `/entities/{entityId}` (clauses 6.5.3.3 and 6.5.3.4).
    #[tokio::test]
    async fn put_and_patch_on_the_entities_collection_are_methods_it_does_not_serve() -> Result<(), Box<dyn Error>> {
        let service = test_api()?.into_service();
        for method in [Method::PUT, Method::PATCH] {
            for path in ["/ngsi-ld/v1/entities", "/ngsi-ld/v1/entities/"] {
                let response = service.clone().oneshot(request(method.clone(), path)?).await?; // clone: the method labels the assertion
                pretty_assertions::assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED, "{method} {path}");
                let allow = response
                    .headers()
                    .get("allow")
                    .and_then(|value| value.to_str().ok())
                    .unwrap_or_default()
                    .to_owned();
                for served in ["GET", "POST", "DELETE"] {
                    assert!(allow.contains(served), "{method} {path}: {allow}");
                }
                assert!(!allow.contains("PUT") && !allow.contains("PATCH"), "{method} {path}: {allow}");
                pretty_assertions::assert_eq!(problem_type(response).await?, "MethodNotAllowed", "{method} {path}");
            }
        }
        Ok(())
    }

    /// A `HEAD` on a resource serving only `POST` is answered the document with
    /// no body: the assembly declares the document's length and strips the
    /// body, as a `HEAD` response must not carry one (RFC 9110, clause 9.3.2).
    #[tokio::test]
    async fn a_head_on_a_post_only_resource_gets_the_document_without_a_body() -> Result<(), Box<dyn Error>> {
        let service = test_api()?.into_service();
        let response = service
            .oneshot(
                Request::builder()
                    .method(Method::HEAD)
                    .uri("/ngsi-ld/v1/entityOperations/create")
                    .body(Body::empty())?,
            )
            .await?;
        pretty_assertions::assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
        let length = response
            .headers()
            .get("content-length")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<usize>().ok());
        assert!(length.is_some_and(|length| length > 0), "a declared document length, found {length:?}");
        let body = to_bytes(response.into_body(), usize::MAX).await?;
        pretty_assertions::assert_eq!(body.len(), 0);
        Ok(())
    }

    /// Metrics are served on the management plane, so the scrape
    /// never answers under `/ngsi-ld/v1` -- neither on the plain data plane nor
    /// in merged mode, where the scrape lives at the root `/metrics` instead.
    #[tokio::test]
    async fn the_data_plane_never_serves_metrics_under_the_api_prefix() -> Result<(), Box<dyn Error>> {
        let plain = test_api()?.into_service().oneshot(request(Method::GET, "/ngsi-ld/v1/metrics")?).await?;
        pretty_assertions::assert_eq!(plain.status(), StatusCode::NOT_FOUND);

        let telemetry = TelemetryRoutes::new(Arc::new(TelemetryRecorder::detached()?))?;
        let merged = HttpApi::assemble(build_routes()?, Some(management_routes(Some(telemetry))), &test_stack()?)
            .into_service()
            .oneshot(request(Method::GET, "/ngsi-ld/v1/metrics")?)
            .await?;
        pretty_assertions::assert_eq!(merged.status(), StatusCode::NOT_FOUND);
        Ok(())
    }

    #[tokio::test]
    async fn merged_mode_mounts_livez_readyz_and_metrics_at_the_root() -> Result<(), Box<dyn Error>> {
        let telemetry = TelemetryRoutes::new(Arc::new(TelemetryRecorder::detached()?))?;
        let service = HttpApi::assemble(build_routes()?, Some(management_routes(Some(telemetry))), &test_stack()?).into_service();
        for path in ["/livez", "/readyz", "/metrics"] {
            let response = service.clone().oneshot(request(Method::GET, path)?).await?;
            assert_ne!(response.status(), StatusCode::NOT_FOUND, "{path} was not routed at the root");
        }
        let nested = service.oneshot(request(Method::GET, "/ngsi-ld/v1/metrics")?).await?;
        pretty_assertions::assert_eq!(nested.status(), StatusCode::NOT_FOUND);
        Ok(())
    }

    /// Merged mode answers a wrong-method request on a root-mounted probe the
    /// same document the data plane and the standalone management plane answer,
    /// so the topology a client sees never changes the refusal's shape.
    #[tokio::test]
    async fn merged_mode_answers_a_wrong_method_on_a_probe_with_the_document() -> Result<(), Box<dyn Error>> {
        let service = HttpApi::assemble(build_routes()?, Some(management_routes(None)), &test_stack()?).into_service();
        let response = service.oneshot(request(Method::POST, "/readyz")?).await?;
        pretty_assertions::assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
        pretty_assertions::assert_eq!(problem_type(response).await?, "MethodNotAllowed");
        Ok(())
    }

    /// The HTTP/3 router must serve the same paths as the TCP service, trailing-
    /// slash normalization included, so a client sees identical routing on either
    /// transport.
    #[tokio::test]
    async fn the_http3_router_normalizes_a_trailing_slash_before_routing() -> Result<(), Box<dyn Error>> {
        let router = test_api()?.h3_router();
        let response = router.oneshot(request(Method::GET, "/ngsi-ld/v1/health/")?).await?;
        assert_ne!(response.status(), StatusCode::NOT_FOUND, "trailing slash was not trimmed for HTTP/3");
        Ok(())
    }

    /// A request under `/ngsi-ld/v1` with `headers` and, for a body-taking
    /// method, an empty JSON object with its declared length.
    fn ngsi_request(method: &Method, path: &str, headers: &[(&str, &str)]) -> Result<Request<Body>, http::Error> {
        let mut request = Request::builder().method(method.clone()).uri(path).header("host", "broker.example"); // clone: the builder owns its method
        for (name, value) in headers {
            request = request.header(*name, *value);
        }
        if [Method::POST, Method::PATCH, Method::PUT].contains(method) {
            request = request.header("content-type", "application/json").header("content-length", "2");
            return request.body(Body::from("{}"));
        }
        request.body(Body::empty())
    }

    /// The `ProblemDetails` `type` of a response, by its last segment.
    async fn problem_type(response: Response) -> Result<String, Box<dyn Error>> {
        let body = to_bytes(response.into_body(), usize::MAX).await?;
        let problem: Value = serde_json::from_slice(&body)?;
        Ok(problem["type"].as_str().and_then(|uri| uri.rsplit('/').next()).unwrap_or_default().to_owned())
    }

    /// One operation of every route family.
    const FAMILY_OPERATIONS: [(Method, &str); 10] = [
        (Method::PATCH, "/ngsi-ld/v1/entities/urn:ngsi-ld:Entity:1/attrs"),
        (Method::POST, "/ngsi-ld/v1/entityOperations/create"),
        (Method::POST, "/ngsi-ld/v1/temporal/entities"),
        (Method::POST, "/ngsi-ld/v1/jsonldContexts"),
        (Method::DELETE, "/ngsi-ld/v1/csourceRegistrations/urn:ngsi-ld:ContextSourceRegistration:1"),
        (Method::POST, "/ngsi-ld/v1/subscriptions"),
        (Method::POST, "/ngsi-ld/v1/csourceSubscriptions"),
        (Method::DELETE, "/ngsi-ld/v1/entityMaps/urn:ngsi-ld:EntityMap:1"),
        (Method::DELETE, "/ngsi-ld/v1/snapshots/urn:ngsi-ld:Snapshot:1"),
        (Method::GET, "/ngsi-ld/v1/info/sourceIdentity"),
    ];

    /// Clause 6.3.20: a parameter an operation does not define is
    /// `InvalidRequest` on every family, Serve @context included, before any
    /// tenant is resolved.
    #[tokio::test]
    async fn every_family_refuses_an_undefined_parameter_as_invalid_request() -> Result<(), Box<dyn Error>> {
        let service = test_api()?.into_service();
        let serve = (Method::GET, "/ngsi-ld/v1/jsonldContexts/urn:ngsi-ld:JsonLdContext:1");
        for (method, path) in FAMILY_OPERATIONS.into_iter().chain([serve]) {
            let response = service.clone().oneshot(ngsi_request(&method, &format!("{path}?bogus=x"), &[])?).await?; // clone: the method labels the assertion
            pretty_assertions::assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{method} {path}");
            pretty_assertions::assert_eq!(problem_type(response).await?, "InvalidRequest", "{method} {path}");
        }
        Ok(())
    }

    /// Clause 6.3.5 and JSON-LD 1.1 API section 9.4.1: two context links are
    /// `BadRequestData` on every family, whatever the verb.
    #[tokio::test]
    async fn every_family_refuses_two_context_links_as_bad_request_data() -> Result<(), Box<dyn Error>> {
        let service = test_api()?.into_service();
        let links = [
            ("link", "<https://contexts.example/a.jsonld>; rel=\"http://www.w3.org/ns/json-ld#context\""),
            ("link", "<https://contexts.example/b.jsonld>; rel=\"http://www.w3.org/ns/json-ld#context\""),
        ];
        for (method, path) in FAMILY_OPERATIONS {
            let response = service.clone().oneshot(ngsi_request(&method, path, &links)?).await?;
            pretty_assertions::assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{method} {path}");
            pretty_assertions::assert_eq!(problem_type(response).await?, "BadRequestData", "{method} {path}");
        }
        Ok(())
    }

    /// Clause 6.2: a configured `apiRoot` is what every NGSI-LD route reads,
    /// its path prefix included, whatever authority the request named.
    #[tokio::test]
    async fn a_configured_api_root_is_the_one_an_ngsi_ld_route_reads() -> Result<(), Box<dyn Error>> {
        let configured = Url::parse("https://proxy.example/broker/")?;
        let probe = Router::new().route(
            "/probe",
            get(|api_root: ApiRoot| async move { api_root.url().map(Url::to_string).unwrap_or_default() }),
        );
        let routed = Router::new().nest(
            "/ngsi-ld/v1",
            test_stack()?.apply_api(ngsi_request_layers(
                probe,
                Some(ConfiguredApiRoot::new(configured.clone())),
                PaginationPolicy::CursorDefault,
            )), // clone: the expectation keeps the URL
        );

        let response = routed.oneshot(ngsi_request(&Method::GET, "/ngsi-ld/v1/probe", &[])?).await?;

        pretty_assertions::assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await?;
        pretty_assertions::assert_eq!(String::from_utf8(body.to_vec())?, configured.to_string());
        Ok(())
    }

    /// Clauses 5.5.9.1 and 5.5.9.2: the policy the assembly installs is the one
    /// every NGSI-LD route pages by.
    #[tokio::test]
    async fn the_installed_pagination_policy_is_the_one_a_route_reads() -> Result<(), Box<dyn Error>> {
        let probe = Router::new().route(
            "/probe",
            get(|RequestPagination { policy, .. }: RequestPagination| async move { format!("{policy:?}") }),
        );
        let routed = Router::new().nest(
            "/ngsi-ld/v1",
            test_stack()?.apply_api(ngsi_request_layers(probe, None, PaginationPolicy::OffsetOnly)),
        );

        let response = routed.oneshot(ngsi_request(&Method::GET, "/ngsi-ld/v1/probe", &[])?).await?;

        pretty_assertions::assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await?;
        pretty_assertions::assert_eq!(String::from_utf8(body.to_vec())?, "OffsetOnly");
        Ok(())
    }
}
