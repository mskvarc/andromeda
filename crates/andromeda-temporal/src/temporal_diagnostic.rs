//! A masked diagnostic for temporal failures the client cannot act on.
//!
//! [`TemporalError::Persistence`](crate::error::TemporalError::Persistence),
//! [`TemporalError::Rendering`](crate::error::TemporalError::Rendering) and
//! [`TemporalError::Internal`](crate::error::TemporalError::Internal) each carry
//! one of these. The text is an operator diagnostic — a persistence adapter's own
//! error, a rendering fault, a broken internal invariant — and is always masked
//! to `internal error` before any client sees it (see `ClassifiedReport::internal`).
//! It stays coarse deliberately: the adapters that raise it live in crates this
//! one cannot depend on without a dependency cycle, so their cause arrives as
//! opaque prose rather than a typed source. There is no generic client-facing
//! escape hatch here; the value never reaches a client.

use std::fmt::Display;
use thiserror::Error;

/// An opaque, operator-facing diagnostic for a masked temporal failure.
#[derive(Debug, Error)]
#[error("{detail}")]
pub struct TemporalDiagnostic {
    /// The diagnostic text, captured from a source that cannot be held typed.
    detail: String,
}

impl TemporalDiagnostic {
    /// Capture a diagnostic from any source that can render itself.
    ///
    /// Takes `impl Display` rather than `impl Into<String>` so a typed source
    /// error can be handed over directly, without a stringly-typed detour at the
    /// call site.
    pub fn new(source: impl Display) -> TemporalDiagnostic {
        TemporalDiagnostic { detail: source.to_string() }
    }
}
