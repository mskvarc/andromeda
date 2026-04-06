//! Query parameter types for subscription endpoints.

use serde::Deserialize;

/// Query parameters for `GET /subscriptions` (list subscriptions).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListParams {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
    pub options: Option<String>,
}
