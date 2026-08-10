use andromeda_http_extraction::tenant_state::TenantStateResolver;
use andromeda_http_headers::ngsild_tenant::NgsiLdTenant;
use andromeda_tenancy::{creation_policy::TenantCreationPolicy, tenant_ref::TenantRef};
use axum::{
    body::Body,
    http::{Request, Response, Uri},
    middleware::Next,
};
use futures::future::BoxFuture;
use headers::{Header, HeaderMapExt};

use crate::{
    api::state::{AndromedaState, BrokerState},
    error::{AndromedaError, ResultInstanceExt},
};
use andromeda_http_response::problem_response::ProblemResponse;

impl TenantStateResolver<AndromedaState> for BrokerState {
    fn resolve_tenant<'a>(
        &'a self,
        tenant: &'a TenantRef,
        creation_policy: TenantCreationPolicy,
        instance: &'a Uri,
    ) -> BoxFuture<'a, Result<AndromedaState, ProblemResponse>> {
        Box::pin(async move {
            let runtime = self
                .tenants
                .resolve_state(tenant, creation_policy)
                .await
                .map_err(AndromedaError::from)
                .with_instance(instance)?;
            Ok((*runtime).clone())
        })
    }
}

pub async fn echo_tenant_header_middleware(request: Request<Body>, next: Next) -> Response<Body> {
    let tenant: Option<NgsiLdTenant> = {
        let mut values = request.headers().get_all(NgsiLdTenant::name()).iter();
        headers::Header::decode(&mut values).ok()
    };

    let mut response = next.run(request).await;

    if let Some(tenant) = tenant {
        response.headers_mut().typed_insert(tenant);
    }

    response
}
