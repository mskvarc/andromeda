//! Handler for the `/version` endpoint.

use crate::{
    error::AndromedaError,
    version::{
        report::{VersionResponse, VersionType},
        service::Version,
    },
};
use andromeda_http_response::problem_response::ProblemResponse;
use andromeda_ngsi_ld_lexicon::uri::{NGSI_LD_URN_NID, Uri};
use axum::{Extension, Json, http::StatusCode, response::IntoResponse};
use urn_rs::UrnBuilder;
use uuid::Uuid;

/// GET /ngsi-ld/v1/version -- Return build and runtime version information.
pub async fn check(Extension(version): Extension<Version>) -> Result<impl IntoResponse, ProblemResponse> {
    let id = UrnBuilder::new(NGSI_LD_URN_NID, &format!("VersionInfo:{}", Uuid::new_v4()))
        .build()
        .map_err(AndromedaError::from)?;

    let version_info = version.check().await.map_err(ProblemResponse::from)?;

    let response = VersionResponse {
        id: Uri::Urn(id),
        type_: VersionType,
        version_info,
    };

    Ok((StatusCode::OK, Json(response)))
}
