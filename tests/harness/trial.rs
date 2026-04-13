#![allow(clippy::expect_used, clippy::unwrap_used)]
use std::{future::Future, panic::AssertUnwindSafe, sync::Mutex};

use futures::FutureExt;
use libtest_mimic::Trial;

use super::{TestCtx, runtime::runtime};

/// Create a Trial that runs an async test function with automatic cleanup.
/// Accepts FnOnce (via Mutex<Option<F>> trick) so closures can move captured values.
pub fn trial<F, Fut>(name: &str, f: F) -> Trial
where
    F: FnOnce(TestCtx) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = ()>,
{
    let f = Mutex::new(Some(f));
    Trial::test(name.to_string(), move || {
        let f = f.lock().unwrap().take().expect("test called more than once");
        runtime().block_on(async {
            super::cleanup::clean_database(super::DATABASE_URL).await;
            let ctx = TestCtx::new();
            let result = AssertUnwindSafe(f(ctx.clone())).catch_unwind().await;
            ctx.cleanup().await;
            match result {
                Ok(()) => Ok(()),
                Err(payload) => std::panic::resume_unwind(payload),
            }
        })
    })
}

/// Create a Trial that is ignored (andromeda divergence from spec).
pub fn trial_ignored<F, Fut>(name: &str, _reason: &str, f: F) -> Trial
where
    F: FnOnce(TestCtx) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = ()>,
{
    trial(name, f).with_ignored_flag(true)
}

/// Create a Trial that runs with a pre-cleaned database.
pub fn trial_clean_db<F, Fut>(name: &str, f: F) -> Trial
where
    F: FnOnce(TestCtx) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = ()>,
{
    let f = Mutex::new(Some(f));
    Trial::test(name.to_string(), move || {
        let f = f.lock().unwrap().take().expect("test called more than once");
        runtime().block_on(async {
            super::cleanup::clean_database(super::DATABASE_URL).await;
            let ctx = TestCtx::new();
            let result = AssertUnwindSafe(f(ctx.clone())).catch_unwind().await;
            ctx.cleanup().await;
            match result {
                Ok(()) => Ok(()),
                Err(payload) => std::panic::resume_unwind(payload),
            }
        })
    })
}
