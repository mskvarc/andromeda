//! Query parameter types for JSON-LD context management endpoints.

use crate::case_insensitive_bool::deserialize_option_bool_case_insensitive;
use serde::Deserialize;

/// Query parameters for `GET /jsonldContexts` (list contexts).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListParams {
    /// The kind of context to list (`kind`).
    pub kind: Option<String>,
    /// Whether the response uses the detailed representation (`details`).
    #[serde(default, deserialize_with = "deserialize_option_bool_case_insensitive")]
    pub details: Option<bool>,
}

/// Query parameters for `GET /jsonldContexts/{contextId}` (serve context).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServeParams {
    /// Whether the response uses the detailed representation (`details`).
    #[serde(default, deserialize_with = "deserialize_option_bool_case_insensitive")]
    pub details: Option<bool>,
}

/// Query parameters for `DELETE /jsonldContexts/{contextId}`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeleteParams {
    /// Whether the context is refetched rather than served from cache (`reload`).
    #[serde(default, deserialize_with = "deserialize_option_bool_case_insensitive")]
    pub reload: Option<bool>,
}
