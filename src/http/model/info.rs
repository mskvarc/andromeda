//! HTTP model types for the `/info` endpoint.

use serde::Serialize;

/// Response body for the context source identity endpoint (section 6.36.3.1).
#[derive(Serialize)]
pub struct ContextSourceIdentity {
    /// The identifier of this context source.
    pub id: String,
    /// Always `"ContextSourceIdentity"`.
    #[serde(rename = "type")]
    pub type_: &'static str,
}
