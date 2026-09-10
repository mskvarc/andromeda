//! The narrow persistence port for periodic invalidation of cached Contexts.
//!
//! Clause 5.13.1 states it plainly: "Implementations shall periodically
//! invalidate the "Cached" @contexts", and clause 6.3.16 repeats it, saying
//! that the invalidation follows the `Expires` field and the
//! `max-age`/`s-maxage` directives the download carried.
//!
//! That is an obligation on the broker, not on any one request. A broker that
//! has stopped downloading new `@contexts` still holds the ones it downloaded
//! before, and they still have to lapse, so invalidation is driven by a clock
//! rather than by the next cache write that happens along.

use crate::error::ContextRegistryError;
use std::{future::Future, pin::Pin};

/// Owned future returned by a cached-Context invalidation implementation.
pub type CachedContextInvalidationFuture<'a> = Pin<Box<dyn Future<Output = Result<InvalidatedContexts, ContextRegistryError>> + Send + 'a>>;

/// How many `"Cached"` `@contexts` one invalidation pass reclaimed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct InvalidatedContexts(u64);

impl InvalidatedContexts {
    /// The outcome of a pass that reclaimed `count` stored Contexts.
    #[must_use]
    pub const fn reclaimed(count: u64) -> InvalidatedContexts {
        InvalidatedContexts(count)
    }

    /// How many stored Contexts the pass reclaimed.
    #[must_use]
    pub const fn count(self) -> u64 {
        self.0
    }

    /// Whether the pass found nothing to reclaim.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

/// Reclaims the `"Cached"` Contexts whose expiration time has passed.
pub trait CachedContextInvalidation: Send + Sync + 'static {
    /// Invalidate every `Cached` Context that has lapsed, reporting how many.
    ///
    /// Contexts of the other two kinds are the client's to delete: clause
    /// 5.13.1 gives an expiration time only to entries the broker downloaded on
    /// its own behalf, and clause 5.13.5 makes removing anything else a client
    /// operation.
    fn invalidate_expired(&self) -> CachedContextInvalidationFuture<'_>;
}

#[cfg(test)]
mod tests {
    use crate::cached_context_invalidation::{CachedContextInvalidationFuture, InvalidatedContexts};

    #[test]
    fn invalidation_future_is_send() {
        fn require_send<T: Send>() {}
        require_send::<CachedContextInvalidationFuture<'static>>();
    }

    /// Clause 5.13.1 asks for the pass, not for a number; the count exists so
    /// an operator can see the pass doing something, and a pass that reclaimed
    /// nothing is the steady state rather than a failure.
    #[test]
    fn a_pass_that_reclaimed_nothing_reports_itself_as_empty() {
        assert!(InvalidatedContexts::reclaimed(0).is_empty());
        assert!(!InvalidatedContexts::reclaimed(3).is_empty());
        pretty_assertions::assert_eq!(InvalidatedContexts::reclaimed(3).count(), 3);
    }
}
