//! `/version` response projection over active component reports.

use andromeda_component_report::ComponentReport;
use andromeda_ngsi_ld_lexicon::{constant_string, uri::Uri};
use serde::Serialize;

constant_string!(ComponentReportListType, "ComponentReportList");

/// Borrowed `/version` response assembled from the active component set.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComponentReportResponse<'a> {
    /// Unique response identifier.
    pub id: Uri,
    /// Response type discriminator.
    #[serde(rename = "type")]
    pub type_: ComponentReportListType,
    /// Exactly the components active in this broker profile.
    pub components: &'a [ComponentReport],
}
