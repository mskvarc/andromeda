//! The lifetimes the resolver assigns where no response states one.
//!
//! Clause 6.3.16 makes a downloaded `@context`'s own `Expires` field and
//! `max-age`/`s-maxage` directives authoritative, so both spans here apply only
//! where no origin server spoke: a download whose response stated nothing, and
//! a document the broker stores itself, which has no origin server at all.

use crate::loader::cache_lifetime::CacheLifetime;

/// The two deployment-configured spans the resolver falls back on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ContextLifetimes {
    /// The heuristic expiration time clause 6.3.16 recommends assigning to a
    /// downloaded `@context` whose response states none.
    pub(crate) heuristic: CacheLifetime,
    /// How long an `@context` the broker stores itself (clause 5.13.1
    /// `"Hosted"` and `"ImplicitlyCreated"`) is used from memory before its row
    /// is read again.
    pub(crate) hosted_revalidation: CacheLifetime,
}
