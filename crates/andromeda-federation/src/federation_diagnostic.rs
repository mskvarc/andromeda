//! A masked diagnostic for federation failures the client cannot act on.
//!
//! [`FederationError::Internal`](crate::error::FederationError::Internal) carries
//! one of these. The text is an operator diagnostic — an HTTP client that would
//! not build, a registration whose `contextSourceInfo` cannot be turned into
//! headers, a broken internal invariant — and is always masked to `internal
//! error` before any client sees it (see `ClassifiedReport::internal`). It stays
//! coarse deliberately: some sources that raise it live in crates this one
//! cannot depend on without a cycle, so their cause arrives as opaque prose
//! rather than a typed source. The value never reaches a client.

use std::fmt::Display;
use thiserror::Error;

/// An opaque, operator-facing diagnostic for a masked federation failure.
#[derive(Debug, Error)]
#[error("{detail}")]
pub struct FederationDiagnostic {
    /// The diagnostic text, captured from a source that cannot be held typed.
    detail: String,
}

impl FederationDiagnostic {
    /// Capture a diagnostic from any source that can render itself.
    ///
    /// Takes `impl Display` rather than `impl Into<String>` so a typed source
    /// error can be handed over directly, without a stringly-typed detour at the
    /// call site.
    pub fn new(source: impl Display) -> FederationDiagnostic {
        FederationDiagnostic { detail: source.to_string() }
    }
}
