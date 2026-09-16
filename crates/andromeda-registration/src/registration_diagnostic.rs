//! A masked diagnostic for registration failures the client cannot act on.
//!
//! [`RegistrationError::Persistence`](crate::error::RegistrationError::Persistence)
//! and [`RegistrationError::Internal`](crate::error::RegistrationError::Internal)
//! both carry one of these. The text is an operator diagnostic — a persistence
//! adapter's own error, a serialization fault on the broker's own output, a
//! broken internal invariant — and is always masked to `internal error` before
//! any client sees it (see `ClassifiedReport::internal`). It stays coarse
//! deliberately: some of the adapters that raise it live in crates this one
//! cannot depend on without a cycle, so their cause arrives as opaque prose
//! rather than a typed source. The value never reaches a client.

use std::fmt::Display;
use thiserror::Error;

/// An opaque, operator-facing diagnostic for a masked registration failure.
#[derive(Debug, Error)]
#[error("{detail}")]
pub struct RegistrationDiagnostic {
    /// The diagnostic text, captured from a source that cannot be held typed.
    detail: String,
}

impl RegistrationDiagnostic {
    /// Capture a diagnostic from any source that can render itself.
    ///
    /// Takes `impl Display` rather than `impl Into<String>` so a typed source
    /// error can be handed over directly, without a stringly-typed detour at the
    /// call site.
    pub fn new(source: impl Display) -> RegistrationDiagnostic {
        RegistrationDiagnostic { detail: source.to_string() }
    }
}
