//! Assembly of named state-complete fragments under the public API root.

use crate::routes::ApiRoutes;
use andromeda_http_extraction::operation_hit_scope::scope_operation_hits;
use andromeda_http_stack::stack::HttpStack;
use axum::{Router, middleware::from_fn};
use tower_http::normalize_path::NormalizePath;

/// Fully assembled, state-complete public HTTP API.
///
/// The NGSI-LD families and the metrics scrape route, when present, all live
/// under `/ngsi-ld/v1`; a scraper therefore targets `/ngsi-ld/v1/metrics`. The
/// scrape route is merged after the operation-hit scope so a scrape is never
/// counted as an NGSI-LD operation.
pub struct HttpApi {
    /// Process-stacked service with path normalization outside Axum routing.
    service: NormalizePath<Router>,
}

impl HttpApi {
    /// Merge every required route family under `/ngsi-ld/v1` and apply the configured stack.
    #[must_use]
    pub fn assemble(routes: ApiRoutes, stack: &HttpStack) -> HttpApi {
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
        // Clause 5.13.3.5: one NGSI-LD request resolves the same `@context`
        // several times internally (write screening, then the write itself);
        // an operation scope makes those internal resolutions count as one use.
        let ngsi = ngsi.layer(from_fn(scope_operation_hits));
        // The scrape route is merged after the operation-hit scope, so a metrics
        // scrape at `/ngsi-ld/v1/metrics` is never counted as an NGSI-LD operation.
        let v1 = match routes.telemetry {
            Some(telemetry) => ngsi.merge(telemetry.into_router()),
            None => ngsi,
        };
        let api_root = Router::new().nest("/ngsi-ld/v1", stack.apply_api(v1));
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

#[cfg(test)]
mod tests {
    use crate::{api::HttpApi, routes::ApiRoutes};
    use andromeda_component_report::{ComponentReport, ComponentVersion};
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
    use andromeda_http_headers::forwarded_header_trust::ForwardedHeaderTrust;
    use andromeda_http_health::routes::HealthRoutes;
    use andromeda_http_notification_receiver::{
        routes::NotificationReceiverRoutes,
        state::{NotificationReceiverResolver, ReceiverResolutionError},
    };
    use andromeda_http_registration::{
        routes::RegistrationRoutes,
        state::{RegistrationRouteServices, RegistrationStateResolver},
    };
    use andromeda_http_snapshot::{
        routes::SnapshotRoutes,
        state::{SnapshotRouteServices, SnapshotStateResolver},
    };
    use andromeda_http_source_identity::{routes::SourceIdentityRoutes, state::SourceIdentityState};
    use andromeda_http_stack::{
        config::{CorsMode, HttpFeatureMode, HttpStackConfig},
        stack::HttpStack,
    };
    use andromeda_http_subscription::{
        routes::SubscriptionRoutes,
        state::{SubscriptionRouteServices, SubscriptionStateResolver},
    };
    use andromeda_http_telemetry::routes::TelemetryRoutes;
    use andromeda_http_temporal::{
        routes::TemporalRoutes,
        state::{TemporalRouteServices, TemporalStateResolver},
    };
    use andromeda_http_tenancy::routes::TenancyRoutes;
    use andromeda_ngsi_ld_lexicon::{context_source_pseudonym::ContextSourcePseudonym, tenant::TenantName, uri::Uri};
    use andromeda_subscription_forwarding::service::SubscriptionForwardingService;
    use andromeda_telemetry::recorder::TelemetryRecorder;
    use andromeda_tenancy::{
        administration::{OrphanTenantEvictor, TenantDirectory},
        context_source_alias::ContextSourceAliases,
        creation_policy::TenantCreationPolicy,
        error::TenancyError,
        tenant_ref::TenantRef,
    };
    use async_trait::async_trait;
    use axum::{
        body::Body,
        http,
        http::{Method, Request, StatusCode},
    };
    use chrono::Utc;
    use futures::future::BoxFuture;
    use std::{error::Error, sync::Arc};
    use tower::ServiceExt;

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

    struct RejectReceiver;

    impl NotificationReceiverResolver for RejectReceiver {
        fn resolve<'a>(&'a self, subscription: &'a Uri) -> BoxFuture<'a, Result<Arc<SubscriptionForwardingService>, ReceiverResolutionError>> {
            Box::pin(async move {
                Err(ReceiverResolutionError::Unknown {
                    subscription: subscription.clone(), // clone: the error owns the identifier
                })
            })
        }
    }

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

    fn test_api() -> Result<HttpApi, Box<dyn Error>> {
        test_api_with_telemetry(None)
    }

    fn test_api_with_telemetry(telemetry: Option<TelemetryRoutes>) -> Result<HttpApi, Box<dyn Error>> {
        let administration = Arc::new(TenantAdministration);
        let directory: Arc<dyn TenantDirectory> = administration.clone();
        let evictor: Arc<dyn OrphanTenantEvictor> = administration;
        let routes = ApiRoutes::builder()
            .entity(EntityRoutes::new(Arc::new(RejectEntity)))
            .temporal(TemporalRoutes::new(Arc::new(RejectTemporal)))
            .context_registry(ContextRegistryRoutes::new(Arc::new(RejectContextRegistry)))
            .registration(RegistrationRoutes::new(Arc::new(RejectRegistration)))
            .subscription(SubscriptionRoutes::new(Arc::new(RejectSubscription)))
            .entity_map(EntityMapRoutes::new(Arc::new(RejectEntityMap)))
            .snapshot(SnapshotRoutes::new(Arc::new(RejectSnapshot)))
            .source_identity(SourceIdentityRoutes::new(SourceIdentityState::new(
                "urn:ngsi-ld:ContextSourceIdentity:test".parse::<Uri>()?,
                ContextSourceAliases::new("test".parse::<ContextSourcePseudonym>()?),
                Utc::now(),
                None,
            )))
            .notification_receiver(NotificationReceiverRoutes::new(Arc::new(RejectReceiver)))
            .tenancy(TenancyRoutes::new(directory, evictor))
            .health(HealthRoutes::new(Health::builder().build()))
            .component_report(ComponentReportRoutes::new(vec![ComponentReport::PostgreSql {
                version: ComponentVersion::from("16.2".to_owned()),
            }]))
            .telemetry(telemetry)
            .build();
        let stack = HttpStack::new(HttpStackConfig {
            request_timeout_secs: 30,
            response_compression: HttpFeatureMode::Disabled,
            server_timing: HttpFeatureMode::Disabled,
            request_decompression: HttpFeatureMode::Disabled,
            cors: CorsMode::Disabled,
            max_request_body_bytes: 1024 * 1024,
            forwarded_headers: ForwardedHeaderTrust::Untrusted,
        });
        Ok(HttpApi::assemble(routes, &stack))
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
            (Method::PATCH, "/ngsi-ld/v1/entities"),
            (Method::PUT, "/ngsi-ld/v1/entities"),
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
            (Method::GET, "/ngsi-ld/v1/subscriptions/urn:ngsi-ld:Subscription:1/stream"),
            (Method::GET, "/ngsi-ld/v1/subscriptions/urn:ngsi-ld:Subscription:1/ws"),
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
            (Method::POST, "/ngsi-ld/v1/tenants/evictOrphans"),
            (Method::GET, "/ngsi-ld/v1/health"),
            (Method::GET, "/ngsi-ld/v1/version"),
        ] {
            let response = service.clone().oneshot(request(method.clone(), path)?).await?;
            assert_ne!(response.status(), StatusCode::NOT_FOUND, "{method} {path} was not routed");
            assert_ne!(response.status(), StatusCode::METHOD_NOT_ALLOWED, "{method} {path} lost its method");
        }
        Ok(())
    }

    #[tokio::test]
    async fn the_metrics_route_is_absent_under_the_api_prefix_unless_telemetry_is_supplied() -> Result<(), Box<dyn Error>> {
        let response = test_api()?.into_service().oneshot(request(Method::GET, "/ngsi-ld/v1/metrics")?).await?;
        pretty_assertions::assert_eq!(response.status(), StatusCode::NOT_FOUND);
        Ok(())
    }

    #[tokio::test]
    async fn the_metrics_route_is_served_under_the_api_prefix_when_supplied() -> Result<(), Box<dyn Error>> {
        let telemetry = TelemetryRoutes::new(Arc::new(TelemetryRecorder::detached()?))?;
        let response = test_api_with_telemetry(Some(telemetry))?
            .into_service()
            .oneshot(request(Method::GET, "/ngsi-ld/v1/metrics")?)
            .await?;
        pretty_assertions::assert_eq!(response.status(), StatusCode::OK);
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

    /// The scrape must no longer answer at the process root now that it is nested.
    #[tokio::test]
    async fn the_metrics_route_is_not_served_at_the_process_root() -> Result<(), Box<dyn Error>> {
        let telemetry = TelemetryRoutes::new(Arc::new(TelemetryRecorder::detached()?))?;
        let response = test_api_with_telemetry(Some(telemetry))?
            .into_service()
            .oneshot(request(Method::GET, "/metrics")?)
            .await?;
        pretty_assertions::assert_eq!(response.status(), StatusCode::NOT_FOUND);
        Ok(())
    }
}
