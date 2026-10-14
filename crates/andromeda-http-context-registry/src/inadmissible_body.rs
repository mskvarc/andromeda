//! The refusal of a `@context` creation body that is not admissible JSON.

use andromeda_json_literal::admission::JsonLiteralError;
use thiserror::Error;

/// A request body the JSON literal admission refused.
#[derive(Debug, Error)]
#[error("request body is not admissible JSON: {0}")]
pub(crate) struct InadmissibleBody(#[source] pub(crate) JsonLiteralError);
