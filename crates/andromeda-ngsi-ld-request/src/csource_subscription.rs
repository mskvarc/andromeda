//! Query parameter types for context source subscription endpoints.

use serde::Deserialize;

/// Query parameters for `GET /csourceSubscriptions` (list context source subscriptions).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListParams {
    /// The page size (clause 5.5.9 `limit`).
    pub limit: Option<u32>,
    /// How many results to skip before the page (`offset`).
    pub offset: Option<u32>,
    /// The legacy representation selector, comma-separated (`options`).
    pub options: Option<String>,
}
