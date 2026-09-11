//! Query parameter types for subscription endpoints.

use crate::{
    error::ParameterError,
    options::ListOptions,
    representation_parameter::{WrittenValue, supported_option},
};
use serde::Deserialize;

/// Query parameters for `GET /subscriptions` (list subscriptions).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListParams {
    /// The page size (clause 5.5.9 `limit`).
    pub limit: Option<u32>,
    /// How many results to skip before the page (`offset`).
    pub offset: Option<u32>,
    /// Typed count and system-attribute response options (`options`).
    pub options: Option<WrittenValue<ListOptions>>,
}

impl ListParams {
    /// The count and system-attribute selection the `options` parameter made.
    ///
    /// # Errors
    ///
    /// Returns [`ParameterError::OptionsInvalid`] for a value clauses 6.3.11
    /// and 6.3.13 do not define, which clause 6.3.20 answers with
    /// `InvalidRequest`.
    pub fn list_options(&self) -> Result<ListOptions, ParameterError> {
        Ok(supported_option(self.options.as_ref())?.unwrap_or_default())
    }
}
