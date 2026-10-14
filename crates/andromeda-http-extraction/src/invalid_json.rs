//! The refusal of a request body whose text is not valid JSON.

use thiserror::Error;

/// A request body that does not parse as a JSON document, or parses into the
/// wrong shape.
#[derive(Debug, Error)]
#[error("invalid JSON: {0}")]
pub struct InvalidJson(#[from] pub serde_json::Error);
