//! The identity of one processed expansion context.
//!
//! A term expanded against a processed context, and a query compiled from such
//! terms, hold only while that processed context is the one the request's
//! `@context` resolves to. The engine processes a context again once a document
//! it was built from is no longer the document the loader resolves (a reload,
//! a deletion, a download whose lifetime lapsed), and the new processed context
//! takes a new identity. An answer kept under the identity of the context it was
//! decided against is therefore never reused past such a change. Identities are
//! drawn from a process-wide counter and never repeat, so a key that outlives
//! its context cannot match the context that replaced it.

use std::sync::atomic::{AtomicU64, Ordering};

/// The identity the next processed expansion context takes.
static NEXT: AtomicU64 = AtomicU64::new(0);

/// The identity of one processed expansion context, unique for the life of the
/// process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ExpansionContextId(u64);

impl ExpansionContextId {
    /// An identity no processed context of this process has taken before.
    pub(crate) fn fresh() -> ExpansionContextId {
        // A read-modify-write hands every caller a distinct value whatever its
        // ordering; nothing else is published through the counter.
        ExpansionContextId(NEXT.fetch_add(1, Ordering::Relaxed))
    }
}

#[cfg(test)]
mod tests {
    use crate::engine::expansion_context_id::ExpansionContextId;

    #[test]
    fn every_fresh_identity_differs_from_the_one_before() {
        let first = ExpansionContextId::fresh();
        let second = ExpansionContextId::fresh();

        assert_ne!(first, second);
    }
}
