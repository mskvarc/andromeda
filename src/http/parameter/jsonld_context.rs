//! Query parameter types for JSON-LD context management endpoints.

use crate::http::parameter::deserialize_option_bool_case_insensitive;
use serde::Deserialize;

/// Query parameters for `GET /jsonldContexts` (list contexts).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListParams {
    pub kind: Option<String>,
    #[serde(default, deserialize_with = "deserialize_option_bool_case_insensitive")]
    pub details: Option<bool>,
}

/// Query parameters for `GET /jsonldContexts/{contextId}` (serve context).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServeParams {
    #[serde(default, deserialize_with = "deserialize_option_bool_case_insensitive")]
    pub details: Option<bool>,
}

/// Query parameters for `DELETE /jsonldContexts/{contextId}`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeleteParams {
    #[serde(default, deserialize_with = "deserialize_option_bool_case_insensitive")]
    pub reload: Option<bool>,
}
