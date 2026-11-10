//! Active-component report operation.

use crate::{
    error::{ComponentReportError, classify},
    report::{ComponentReportListType, ComponentReportResponse},
    state::ComponentReportState,
};
use andromeda_http_response::problem_response::ProblemResponse;
use andromeda_ngsi_ld_lexicon::{identifier::Identifier, ngsi_ld_urn::ComponentReportListId};
use axum::{
    Json,
    extract::{OriginalUri, State},
    response::{IntoResponse, Response},
};

/// Return exactly the components selected by broker composition.
///
/// # Errors
///
/// Returns a [`ProblemResponse`] when the component-report response identifier
/// cannot be minted.
pub async fn get(State(state): State<ComponentReportState>, OriginalUri(uri): OriginalUri) -> Result<Response, ProblemResponse> {
    let id = ComponentReportListId::mint()
        .map(Identifier::from)
        .map_err(|error| ProblemResponse::classified(classify(&ComponentReportError::from(error)), uri))?;
    Ok(Json(ComponentReportResponse {
        id,
        type_: ComponentReportListType,
        components: state.components(),
    })
    .into_response())
}
