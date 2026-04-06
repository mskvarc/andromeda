//! Query parameter types for context source subscription endpoints.

use serde::Deserialize;

/// Query parameters for `GET /csourceSubscriptions` (list context source subscriptions).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListParams {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
    pub page: Option<i64>,
    pub options: Option<String>,
}
