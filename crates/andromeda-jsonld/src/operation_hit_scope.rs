//! Per-operation deduplication of clause 5.13.3.5 context-access counting.
//!
//! Clause 5.13.3.5 defines `numberOfHits` as the "number of times the @context
//! was found in the storage". A single NGSI-LD operation -- one HTTP request, or
//! the processing of one committed change into notifications -- resolves the
//! same `@context` several times internally: an entity write is screened
//! (clause 5.5.4) and then expanded again for storage, and a notification is
//! compacted against the subscription's context. Counting each internal
//! resolution separately reports a number that reflects the broker's internal
//! plumbing rather than the client-visible "times the context was used", and no
//! two brokers plumb it alike.
//!
//! An operation scope collapses those internal resolutions: the first access of
//! a given context URL within the scope counts, and the rest are recognised as
//! the same use. A resolution that happens outside any scope -- a maintenance
//! task, a test -- counts every time, which is the conservative reading.
//!
//! The scope is task-local rather than threaded through every expansion call
//! because the resolutions it unifies are spread across layers that do not share
//! a call signature (transport screening, the entity service, the notification
//! processor). It is established at the two operation boundaries that own those
//! layers and read here by the resolver.

use iri_rs::IriBuf;
use std::{cell::RefCell, collections::HashSet, future::Future};

tokio::task_local! {
    /// Context URLs already counted in the current operation.
    static OPERATION_HITS: RefCell<HashSet<IriBuf>>;
}

/// Whether a context access is the first of its URL in the current operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HitAccounting {
    /// The access is the first for this URL in the scope, so it is counted.
    Count,
    /// The URL was already counted in this operation; the access is folded in.
    AlreadyCounted,
}

/// Run `future` within a fresh operation scope.
///
/// Every context access made while the future runs on this task counts at most
/// once per URL. Nesting re-scopes: an inner call establishes its own fresh set,
/// which is the correct reading when a boundary is entered directly rather than
/// through an outer one.
pub async fn run_in_operation_scope<F>(future: F) -> F::Output
where
    F: Future,
{
    OPERATION_HITS.scope(RefCell::new(HashSet::new()), future).await
}

/// Record an access and report whether it is the first for `url` in this scope.
///
/// Outside any scope every access reports [`HitAccounting::Count`], so a
/// resolution that is not part of a bounded operation is counted as it happens.
pub(crate) fn record_access(url: &IriBuf) -> HitAccounting {
    OPERATION_HITS
        .try_with(|seen| {
            if seen.borrow().contains(url) {
                HitAccounting::AlreadyCounted
            } else {
                seen.borrow_mut().insert(url.clone()); // clone: the scope set owns the URL for the operation's lifetime
                HitAccounting::Count
            }
        })
        .unwrap_or(HitAccounting::Count)
}

#[cfg(test)]
mod tests {
    use crate::operation_hit_scope::{HitAccounting, record_access, run_in_operation_scope};
    use iri_rs::IriBuf;
    use pretty_assertions::assert_eq;

    fn url(text: &str) -> IriBuf {
        IriBuf::new(text.to_string()).expect("valid URL")
    }

    #[tokio::test]
    async fn the_first_access_of_a_url_counts_and_the_rest_do_not() {
        run_in_operation_scope(async {
            let context = url("https://example.com/ctx.jsonld");
            let other = url("https://example.com/other.jsonld");

            assert_eq!(record_access(&context), HitAccounting::Count);
            assert_eq!(record_access(&context), HitAccounting::AlreadyCounted);
            assert_eq!(record_access(&context), HitAccounting::AlreadyCounted);
            // A different URL is a distinct use and counts on its own.
            assert_eq!(record_access(&other), HitAccounting::Count);
        })
        .await;
    }

    #[tokio::test]
    async fn a_fresh_scope_counts_the_same_url_again() {
        let context = url("https://example.com/ctx.jsonld");
        run_in_operation_scope(async {
            assert_eq!(record_access(&context), HitAccounting::Count);
        })
        .await;
        run_in_operation_scope(async {
            // A separate operation resolving the same context is a separate use.
            assert_eq!(record_access(&context), HitAccounting::Count);
        })
        .await;
    }

    #[tokio::test]
    async fn outside_a_scope_every_access_counts() {
        let context = url("https://example.com/ctx.jsonld");
        assert_eq!(record_access(&context), HitAccounting::Count);
        assert_eq!(record_access(&context), HitAccounting::Count);
    }
}
