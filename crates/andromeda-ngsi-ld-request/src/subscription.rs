//! Query parameter types for subscription endpoints.

use serde::Deserialize;

/// Query parameters for `GET /subscriptions` (list subscriptions).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListParams {
    /// The page size (clause 5.5.9 `limit`).
    pub limit: Option<i64>,
    /// How many results to skip before the page (`offset`).
    pub offset: Option<i64>,
    /// The legacy representation selector, comma-separated (`options`).
    pub options: Option<String>,
}
