//! Active-component report operation.

use crate::{
    error::{ComponentReportError, classify},
    report::{ComponentReportListType, ComponentReportResponse},
    state::ComponentReportState,
};
use andromeda_http_response::problem_response::ProblemResponse;
use andromeda_ngsi_ld_lexicon::{identifier::Identifier, uri::NGSI_LD_URN_NID};
use axum::{
    Json,
    extract::{OriginalUri, State},
    response::{IntoResponse, Response},
};
use urn_rs::UrnBuilder;
use uuid::Uuid;

/// Return exactly the components selected by broker composition.
///
/// # Errors
///
/// Returns a [`ProblemResponse`] when the component-report response identifier
/// cannot be built as an NGSI-LD URN.
pub async fn get(State(state): State<ComponentReportState>, OriginalUri(uri): OriginalUri) -> Result<Response, ProblemResponse> {
    let id = UrnBuilder::new(NGSI_LD_URN_NID, &format!("ComponentReportList:{}", Uuid::new_v4()))
        .build()
        .map_err(ComponentReportError::from)
        .and_then(|urn| Identifier::try_from(urn).map_err(ComponentReportError::from))
        .map_err(|error| classify(&error).with_instance(uri))?;
    Ok(Json(ComponentReportResponse {
        id,
        type_: ComponentReportListType,
        components: state.components(),
    })
    .into_response())
}
