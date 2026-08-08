//! Query parameter types for context source subscription endpoints.

use crate::options::{ListOptions, deserialize_list_options};
use serde::Deserialize;

/// Query parameters for `GET /csourceSubscriptions` (list context source subscriptions).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListParams {
    /// The page size (clause 5.5.9 `limit`).
    pub limit: Option<u32>,
    /// How many results to skip before the page (`offset`).
    pub offset: Option<u32>,
    /// Typed count and system-attribute response options (`options`).
    #[serde(default, deserialize_with = "deserialize_list_options")]
    pub options: Option<ListOptions>,
}
