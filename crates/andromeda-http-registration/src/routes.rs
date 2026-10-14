//! Typed, state-complete inventory of context-source registration routes.

use crate::{
    handler,
    state::{RegistrationRouteState, RegistrationStateResolver},
};
use andromeda_http_extraction::accept_language::AcceptLanguageReading;
use axum::{
    Router,
    routing::{get, post},
};
use std::sync::Arc;

/// The complete context-source registration route fragment.
pub struct RegistrationRoutes {
    router: Router,
}

impl RegistrationRoutes {
    /// Construct every registration route with its focused resolver, Query
    /// Context Source Registrations reading `Accept-Language` as
    /// `accept_language` says.
    pub fn new(resolver: Arc<dyn RegistrationStateResolver>, accept_language: AcceptLanguageReading) -> RegistrationRoutes {
        let registration_routes = Router::new()
            .route("/", post(handler::create))
            .route("/", accept_language.negotiate(get(handler::query)))
            .route("/{registration_id}", get(handler::get).patch(handler::update).delete(handler::delete));
        let router = Router::new()
            .nest("/csourceRegistrations", registration_routes)
            .with_state(RegistrationRouteState::new(resolver));
        RegistrationRoutes { router }
    }

    /// Consume the state-complete route fragment.
    pub fn into_router(self) -> Router {
        self.router
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        routes::RegistrationRoutes,
        state::{RegistrationRouteServices, RegistrationStateResolver},
    };
    use andromeda_http_extraction::accept_language::AcceptLanguageReading;
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

    impl RegistrationStateResolver for RejectingResolver {
        fn resolve<'a>(
            &'a self,
            _tenant: &'a TenantRef,
            _creation_policy: TenantCreationPolicy,
        ) -> BoxFuture<'a, Result<Arc<RegistrationRouteServices>, TenancyError>> {
            Box::pin(async { Err(TenancyError::MultiTenancyDisabled) })
        }
    }

    #[tokio::test]
    async fn owns_the_complete_registration_inventory() -> Result<(), Box<dyn Error>> {
        let routes = RegistrationRoutes::new(Arc::new(RejectingResolver), AcceptLanguageReading::Ignored).into_router();
        for (method, path) in [
            (Method::POST, "/csourceRegistrations"),
            (Method::GET, "/csourceRegistrations"),
            (Method::GET, "/csourceRegistrations/urn:ngsi-ld:ContextSourceRegistration:1"),
            (Method::PATCH, "/csourceRegistrations/urn:ngsi-ld:ContextSourceRegistration:1"),
            (Method::DELETE, "/csourceRegistrations/urn:ngsi-ld:ContextSourceRegistration:1"),
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
