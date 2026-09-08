//! Typed, state-complete inventory of subscription routes.

use crate::{
    csource_subscription,
    sse,
    state::{SubscriptionRouteState, SubscriptionStateResolver},
    subscription,
    ws,
};
use andromeda_http_extraction::{scoped_response::echo_scope_headers, snapshot_scope_guard::reject_snapshot_scope};
use axum::{
    Router,
    middleware::from_fn,
    routing::{get, post},
};
use std::sync::Arc;

/// The complete entity and context-source subscription route fragment.
pub struct SubscriptionRoutes {
    router: Router,
}

impl SubscriptionRoutes {
    /// Construct subscription lifecycle and streaming routes with focused state.
    pub fn new(resolver: Arc<dyn SubscriptionStateResolver>) -> SubscriptionRoutes {
        let subscriptions = Router::new()
            .route("/", post(subscription::create).get(subscription::query))
            .route(
                "/{subscription_id}",
                get(subscription::get).patch(subscription::update).delete(subscription::delete),
            )
            .route("/{subscription_id}/stream", get(sse::stream))
            .route("/{subscription_id}/ws", get(ws::stream))
            .layer(from_fn(echo_scope_headers));
        let csource_subscriptions = Router::new()
            .route("/", post(csource_subscription::create).get(csource_subscription::query))
            .route(
                "/{subscription_id}",
                get(csource_subscription::get)
                    .patch(csource_subscription::update)
                    .delete(csource_subscription::delete),
            )
            .layer(from_fn(reject_snapshot_scope));
        let router = Router::new()
            .nest("/subscriptions", subscriptions)
            .nest("/csourceSubscriptions", csource_subscriptions)
            .with_state(SubscriptionRouteState::new(resolver));
        SubscriptionRoutes { router }
    }

    /// Consume the state-complete route fragment.
    pub fn into_router(self) -> Router {
        self.router
    }
}

#[cfg(test)]
mod tests {
    use crate::state::{SubscriptionRouteServices, SubscriptionStateResolver};
    use andromeda_tenancy::{creation_policy::TenantCreationPolicy, error::TenancyError, tenant_ref::TenantRef};
    use axum::{
        body::Body,
        http::{Method, Request, StatusCode},
    };
    use futures::future::BoxFuture;
    use std::{error::Error, sync::Arc};
    use tower::ServiceExt;

    struct RejectingResolver;

    impl SubscriptionStateResolver for RejectingResolver {
        fn resolve<'a>(
            &'a self,
            _tenant: &'a TenantRef,
            _creation_policy: TenantCreationPolicy,
        ) -> BoxFuture<'a, Result<Arc<SubscriptionRouteServices>, TenancyError>> {
            Box::pin(async { Err(TenancyError::MultiTenancyDisabled) })
        }
    }

    #[tokio::test]
    async fn owns_subscription_and_streaming_route_inventory() -> Result<(), Box<dyn Error>> {
        let routes = super::SubscriptionRoutes::new(Arc::new(RejectingResolver)).into_router();
        for (method, path) in [
            (Method::POST, "/subscriptions"),
            (Method::GET, "/subscriptions"),
            (Method::GET, "/subscriptions/urn:ngsi-ld:Subscription:1"),
            (Method::PATCH, "/subscriptions/urn:ngsi-ld:Subscription:1"),
            (Method::DELETE, "/subscriptions/urn:ngsi-ld:Subscription:1"),
            (Method::GET, "/subscriptions/urn:ngsi-ld:Subscription:1/stream"),
            (Method::GET, "/subscriptions/urn:ngsi-ld:Subscription:1/ws"),
            (Method::POST, "/csourceSubscriptions"),
            (Method::GET, "/csourceSubscriptions"),
            (Method::GET, "/csourceSubscriptions/urn:ngsi-ld:Subscription:2"),
            (Method::PATCH, "/csourceSubscriptions/urn:ngsi-ld:Subscription:2"),
            (Method::DELETE, "/csourceSubscriptions/urn:ngsi-ld:Subscription:2"),
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
