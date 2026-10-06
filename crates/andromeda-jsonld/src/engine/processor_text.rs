//! A JSON text the JSON-LD processor refused to read.

use jstrict::parse::Error as ProcessorParseError;
use thiserror::Error;

/// The processor's reader refused a JSON text the broker handed it.
///
/// The broker hands the processor only text it wrote itself or admitted, so
/// this reports a defect rather than a client mistake.
#[derive(Debug, Error)]
#[error(transparent)]
pub struct ProcessorTextError(#[from] ProcessorParseError);
