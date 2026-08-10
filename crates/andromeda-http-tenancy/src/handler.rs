//! Tenant administration operations.

use crate::{
    error::{TenancyHttpError, classify},
    state::TenancyRouteState,
};
use andromeda_http_extraction::query::ValidatedQuery;
use andromeda_http_response::problem_response::ProblemResponse;
use andromeda_ngsi_ld_lexicon::uri::{NGSI_LD_URN_NID, Uri};
use andromeda_ngsi_ld_model::tenant_list::{TenantList, TenantListType};
use andromeda_ngsi_ld_request::empty::EmptyParams;
use axum::{
    Json,
    extract::{OriginalUri, State},
    response::IntoResponse,
};
use serde::Serialize;
use urn_rs::UrnBuilder;
use uuid::Uuid;

/// Wire response for an explicit orphan-runtime sweep.
#[derive(Debug, Serialize)]
pub struct OrphanEvictionResponse {
    /// Number of tenant runtimes released by the sweep.
    evicted: usize,
}

/// Query every registered named tenant.
pub async fn query(
    State(state): State<TenancyRouteState>,
    OriginalUri(uri): OriginalUri,
    ValidatedQuery(_params): ValidatedQuery<EmptyParams>,
) -> Result<impl IntoResponse, ProblemResponse> {
    let id = UrnBuilder::new(NGSI_LD_URN_NID, &format!("TenantList:{}", Uuid::new_v4()))
        .build()
        .map_err(TenancyHttpError::from)
        .map_err(|error| classify(&error).with_instance(uri.clone()))?;
    let tenant_list = state
        .directory()
        .registered_tenants()
        .await
        .map_err(TenancyHttpError::from)
        .map_err(|error| classify(&error).with_instance(uri))?;
    Ok(Json(TenantList {
        id: Uri::Urn(id),
        type_: TenantListType,
        tenant_list,
    }))
}

/// Force one eviction sweep for cached runtimes whose tenant was removed.
pub async fn evict_orphans(State(state): State<TenancyRouteState>) -> impl IntoResponse {
    Json(OrphanEvictionResponse {
        evicted: state.evictor().evict_orphan_runtimes().await,
    })
}
