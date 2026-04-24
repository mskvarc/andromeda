//! Handler for the `/tenants` endpoint.

use crate::{
    api::state::AppState,
    core::error::{AndromedaError, ResultInstanceExt},
    domain::{
        tenant::{TenantList, TenantListType},
        uri::{NGSI_LD_URN_NID, Uri},
    },
    http::{error::problem::NgsiLdError, extractor::query::ValidatedQuery, parameter::EmptyParams},
};
use axum::{
    Json,
    extract::{OriginalUri, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde_json::json;
use urn_rs::UrnBuilder;
use uuid::Uuid;

/// GET /ngsi-ld/v1/tenants -- Query available named tenants as pure JSON.
pub async fn query(
    State(app): State<AppState>,
    OriginalUri(uri): OriginalUri,
    ValidatedQuery(_params): ValidatedQuery<EmptyParams>,
) -> Result<impl IntoResponse, NgsiLdError> {
    let id = UrnBuilder::new(NGSI_LD_URN_NID, &format!("TenantList:{}", Uuid::new_v4()))
        .build()
        .map_err(AndromedaError::from)?;
    let tenant_list = app.tenants.list_named_tenants().await.with_instance(&uri)?;

    let response = TenantList {
        id: Uri::Urn(id),
        type_: TenantListType::TenantList,
        tenant_list,
    };

    Ok(Json(response))
}

/// POST /ngsi-ld/v1/tenants/evictOrphans -- Andromeda-specific admin
/// endpoint. Forces an immediate sweep of tenant runtimes whose registry
/// row has been removed (e.g., after external cleanup), closing their
/// pools and returning the PostgreSQL connection slots. Returns the number
/// of runtimes evicted.
pub async fn evict_orphans(State(app): State<AppState>) -> impl IntoResponse {
    let evicted = app.tenants.evict_orphan_runtimes().await;
    (StatusCode::OK, Json(json!({ "evicted": evicted })))
}
