//! The failure to decode a request's query string into a parameter set.

use serde_urlencoded::de::Error as UrlencodedError;
use thiserror::Error;

/// A query string the parameter set could not decode.
#[derive(Debug, Error)]
#[error("invalid query parameters: {0}")]
pub struct QueryDecodeError(#[from] pub UrlencodedError);
