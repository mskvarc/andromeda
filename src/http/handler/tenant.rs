//! Handler for the `/tenants` endpoint.

use crate::{
    api::state::BrokerState,
    error::{AndromedaError, ResultInstanceExt},
};
use andromeda_http_extraction::query::ValidatedQuery;
use andromeda_http_response::problem_response::ProblemResponse;
use andromeda_ngsi_ld_lexicon::uri::{NGSI_LD_URN_NID, Uri};
use andromeda_ngsi_ld_model::tenant_list::{TenantList, TenantListType};
use andromeda_ngsi_ld_request::empty::EmptyParams;
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
    State(broker): State<BrokerState>,
    OriginalUri(uri): OriginalUri,
    ValidatedQuery(_params): ValidatedQuery<EmptyParams>,
) -> Result<impl IntoResponse, ProblemResponse> {
    let id = UrnBuilder::new(NGSI_LD_URN_NID, &format!("TenantList:{}", Uuid::new_v4()))
        .build()
        .map_err(AndromedaError::from)?;
    let tenant_list = broker.tenants.registered_tenants().await.map_err(AndromedaError::from).with_instance(&uri)?;

    let response = TenantList {
        id: Uri::Urn(id),
        type_: TenantListType,
        tenant_list,
    };

    Ok(Json(response))
}

/// POST /ngsi-ld/v1/tenants/evictOrphans -- Andromeda-specific admin
/// endpoint. Forces an immediate sweep of tenant runtimes whose registry
/// row has been removed (e.g., after external cleanup), closing their
/// pools and returning the PostgreSQL connection slots. Returns the number
/// of runtimes evicted.
pub async fn evict_orphans(State(broker): State<BrokerState>) -> impl IntoResponse {
    let evicted = broker.tenants.evict_orphan_runtimes().await;
    (StatusCode::OK, Json(json!({ "evicted": evicted })))
}
