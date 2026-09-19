//! Query parameter types for subscription endpoints.

use crate::{
    error::ParameterError,
    options::ListOptions,
    pagination_cursor::validate_cursor_offset_exclusion,
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
    /// The opaque, keyset-backed pagination pointer (clause 5.5.9.1).
    ///
    /// A broker-emitted token carried inside a `prev`/`next`/`last` `Link`; the
    /// client only passes it back by following the link and never authors or
    /// reads it. Declared so `#[serde(deny_unknown_fields)]` admits it and its
    /// mutual exclusion with `offset` can be checked. Its contents are opaque
    /// (see `docs/conformance/stable-pagination.md`).
    pub cursor: Option<String>,
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
    /// `InvalidRequest`, and [`ParameterError::CursorWithOffset`] when an opaque
    /// `cursor` (clause 5.5.9.1) is combined with an `offset` (clause 5.5.9.2).
    pub fn list_options(&self) -> Result<ListOptions, ParameterError> {
        validate_cursor_offset_exclusion(self.cursor.as_deref(), self.offset)?;
        Ok(supported_option(self.options.as_ref())?.unwrap_or_default())
    }
}
