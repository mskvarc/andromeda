//! Per-batch memory of the `@context` references that failed to resolve.
//!
//! Every entity of an `application/ld+json` batch carries its own `@context`,
//! and the batch admits each distinct one on its own. Contexts written
//! differently can still name one and the same remote document, so a document
//! that cannot be retrieved would be requested again for every context naming
//! it, each attempt waiting out the same failure, while clause 5.5.6 answers
//! every one of them alike with `LdContextNotAvailable`. Within a batch scope
//! the first failure of a reference is remembered, and every later resolution
//! of that reference in the scope fails with it, without another attempt.
//!
//! Only failures are remembered: a resolution that succeeded is held by the
//! loader's own cache, for the lifetime clause 6.3.16 gives it. A failure is
//! never kept past its scope, so the next request tries the document again.
//!
//! The scope is task-local because the resolutions happen deep inside the
//! JSON-LD processor, which knows nothing of the batch it serves. Outside a
//! scope every resolution is attempted.

use crate::loader::error::LoaderError;
use iri_rs::IriBuf;
use std::{cell::RefCell, collections::HashMap, future::Future, sync::Arc};

tokio::task_local! {
    /// The failure each reference resolved to so far in the batch running on
    /// this task.
    static BATCH_FAILURES: RefCell<HashMap<IriBuf, Arc<LoaderError>>>;
}

/// Run `batch` within a fresh batch scope.
///
/// A reference that fails to resolve while the future runs on this task fails
/// every later resolution of it in the scope with the same failure. Nesting
/// re-scopes: an inner scope starts with nothing remembered.
///
/// The scope wraps `batch` in place rather than awaiting it from an `async fn`,
/// which would hold the future twice: once as the argument and once as the
/// awaited value.
pub fn run_in_batch_scope<F>(batch: F) -> impl Future<Output = F::Output>
where
    F: Future,
{
    BATCH_FAILURES.scope(RefCell::new(HashMap::new()), batch)
}

/// Resolve `reference` by running `resolve`, unless it already failed in the
/// current batch scope.
///
/// A reference that already failed returns that failure and `resolve` is never
/// run; a failure of `resolve` is remembered for the rest of the scope. The
/// resolution is taken as a closure so its future is built in place, not
/// carried in as an argument and held twice.
///
/// # Errors
///
/// Returns the failure remembered for `reference`, or the one `resolve` fails
/// with.
pub(crate) async fn resolve_within_batch<T>(reference: &IriBuf, resolve: impl AsyncFnOnce() -> Result<T, Arc<LoaderError>>) -> Result<T, Arc<LoaderError>> {
    if let Some(failure) = remembered_failure(reference) {
        return Err(failure);
    }
    let outcome = resolve().await;
    if let Err(failure) = &outcome {
        remember_failure(reference, failure);
    }
    outcome
}

/// The failure `reference` resolved to earlier in the current batch scope.
fn remembered_failure(reference: &IriBuf) -> Option<Arc<LoaderError>> {
    // Outside a batch scope nothing is remembered.
    BATCH_FAILURES
        .try_with(|failures| failures.borrow().get(reference).map(Arc::clone))
        .ok()
        .flatten()
}

/// Remember that `reference` failed with `failure` for the rest of the current
/// batch scope; a no-op outside one.
///
/// The first failure stays: a resolution that started before it was
/// remembered and failed after does not replace it.
fn remember_failure(reference: &IriBuf, failure: &Arc<LoaderError>) {
    let remembered = BATCH_FAILURES.try_with(|failures| {
        failures
            .borrow_mut()
            .entry(reference.clone()) // clone: the scope owns the reference for the rest of the batch
            .or_insert_with(|| Arc::clone(failure));
    });
    match remembered {
        Ok(()) => {}
        // A failure outside a batch scope is not remembered.
        Err(_outside_a_batch) => {}
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        batch_failure_scope::{resolve_within_batch, run_in_batch_scope},
        loader::error::LoaderError,
    };
    use iri_rs::IriBuf;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    fn reference(text: &str) -> IriBuf {
        IriBuf::new(text.to_owned()).expect("a valid IRI")
    }

    /// A resolution of `url` that fails, counted in `attempts` only once it
    /// runs.
    fn failing<'a>(url: &'a IriBuf, attempts: &'a AtomicUsize) -> impl AsyncFnOnce() -> Result<(), Arc<LoaderError>> + 'a {
        async move || {
            attempts.fetch_add(1, Ordering::Relaxed);
            Err(Arc::new(LoaderError::fetch_disabled(url.clone())))
        }
    }

    /// A resolution that succeeds, counted in `attempts` only once it runs.
    fn succeeding(attempts: &AtomicUsize) -> impl AsyncFnOnce() -> Result<(), Arc<LoaderError>> + '_ {
        async move || {
            attempts.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }
    }

    #[tokio::test]
    async fn a_reference_that_failed_in_the_batch_fails_again_without_another_attempt() {
        let url = reference("https://example.com/unreachable.jsonld");
        let attempts = AtomicUsize::new(0);

        let (first, second) = run_in_batch_scope(async {
            let first = resolve_within_batch(&url, failing(&url, &attempts)).await;
            let second = resolve_within_batch(&url, failing(&url, &attempts)).await;
            (first, second)
        })
        .await;

        pretty_assertions::assert_eq!(attempts.load(Ordering::Relaxed), 1);
        let (Err(first), Err(second)) = (first, second) else {
            panic!("both resolutions fail");
        };
        assert!(Arc::ptr_eq(&first, &second), "the second resolution fails with the remembered failure");
    }

    #[tokio::test]
    async fn a_failure_of_one_reference_leaves_another_to_be_attempted() {
        let unreachable = reference("https://example.com/unreachable.jsonld");
        let other = reference("https://example.com/other.jsonld");
        let attempts = AtomicUsize::new(0);

        let other_outcome = run_in_batch_scope(async {
            resolve_within_batch(&unreachable, failing(&unreachable, &attempts))
                .await
                .expect_err("the reference fails");
            resolve_within_batch(&other, succeeding(&attempts)).await
        })
        .await;

        pretty_assertions::assert_eq!(attempts.load(Ordering::Relaxed), 2);
        assert!(other_outcome.is_ok(), "{other_outcome:?}");
    }

    #[tokio::test]
    async fn a_success_is_not_remembered_by_the_batch() {
        let url = reference("https://example.com/terms.jsonld");
        let attempts = AtomicUsize::new(0);

        run_in_batch_scope(async {
            resolve_within_batch(&url, succeeding(&attempts)).await.expect("the first resolution");
            resolve_within_batch(&url, succeeding(&attempts)).await.expect("the second resolution");
        })
        .await;

        pretty_assertions::assert_eq!(attempts.load(Ordering::Relaxed), 2);
    }

    #[tokio::test]
    async fn a_fresh_batch_attempts_the_reference_again() {
        let url = reference("https://example.com/unreachable.jsonld");
        let attempts = AtomicUsize::new(0);

        for _ in 0..2 {
            run_in_batch_scope(resolve_within_batch(&url, failing(&url, &attempts)))
                .await
                .expect_err("the reference fails");
        }

        pretty_assertions::assert_eq!(attempts.load(Ordering::Relaxed), 2);
    }

    #[tokio::test]
    async fn outside_a_batch_every_resolution_is_attempted() {
        let url = reference("https://example.com/unreachable.jsonld");
        let attempts = AtomicUsize::new(0);

        resolve_within_batch(&url, failing(&url, &attempts))
            .await
            .expect_err("the first resolution fails");
        resolve_within_batch(&url, failing(&url, &attempts))
            .await
            .expect_err("the second resolution fails");

        pretty_assertions::assert_eq!(attempts.load(Ordering::Relaxed), 2);
    }
}
