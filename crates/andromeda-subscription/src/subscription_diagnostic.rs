//! A masked diagnostic for subscription failures the client cannot act on.
//!
//! [`SubscriptionError::Persistence`](crate::error::SubscriptionError::Persistence)
//! and [`SubscriptionError::Internal`](crate::error::SubscriptionError::Internal)
//! both carry one of these. The text is an operator diagnostic — a persistence
//! adapter's own error, a broken internal invariant, a fixture that would not
//! compile — and is always masked to `internal error` before any client sees it
//! (see `ClassifiedReport::internal`). It stays coarse deliberately: the sources
//! that raise it live in crates this one cannot depend on without a dependency
//! cycle, so their cause arrives as opaque prose rather than a typed source.
//! There is no generic client-facing escape hatch here; the value never reaches
//! a client.

use std::fmt::Display;
use thiserror::Error;

/// An opaque, operator-facing diagnostic for a masked subscription failure.
#[derive(Debug, Error)]
#[error("{detail}")]
pub struct SubscriptionDiagnostic {
    /// The diagnostic text, captured from a source that cannot be held typed.
    detail: String,
}

impl SubscriptionDiagnostic {
    /// Capture a diagnostic from any source that can render itself.
    ///
    /// Takes `impl Display` rather than `impl Into<String>` so a typed source
    /// error can be handed over directly, without a stringly-typed detour at the
    /// call site.
    pub fn new(source: impl Display) -> SubscriptionDiagnostic {
        SubscriptionDiagnostic { detail: source.to_string() }
    }
}
