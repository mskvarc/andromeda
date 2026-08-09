//! The single failure a `Link` header can report.

use thiserror::Error;

/// Error returned when a `Link` header cannot be parsed.
///
/// Carries no detail: RFC 8288 gives a server no way to report *which* part of
/// a `Link` header it rejected, so every caller in this tree maps the failure
/// to the same outcome and none of them inspects it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("invalid Link header")]
pub struct LinkParseError;
