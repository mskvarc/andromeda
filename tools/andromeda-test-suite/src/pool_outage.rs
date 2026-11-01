//! A tenant database's pool killed at the pooler for a second and resumed, and requests sent through the outage until
//! the tenant serves again.
//!
//! While the pool is killed the pooler drops the broker's connections and fails new logins, and it keeps the failed
//! login for `server_login_retry` (15 s by default) after the resume, so the tenant may take that long, plus the
//! broker's own acquire deadline, to serve again.

use crate::{
    database_name::DatabaseName,
    load_outcome::{LoadOutcome, RetryAfter},
    pooler_console::{PoolerConsole, PoolerConsoleError},
};
use http::StatusCode;
use std::{future::Future, time::Duration};
use tokio::time::{Instant, sleep};

/// How long the pool stays killed.
pub const KILLED_FOR: Duration = Duration::from_secs(1);

/// The longest requests are sent through the outage before the tenant must serve again.
pub const RECOVERY_BOUND: Duration = Duration::from_secs(45);

/// The pause between two requests.
const REQUEST_SPACING: Duration = Duration::from_millis(200);

/// Whether `answer` is one an outage may give a write: the write landed with one of `landed`, or 503 asking to retry
/// after a second (a pool that handed out no connection in time) or five (a store that could not be reached), or no
/// answer at all.
#[must_use]
pub fn landed_or_paced(answer: &LoadOutcome, landed: &[StatusCode]) -> bool {
    match answer {
        LoadOutcome::Answered {
            status: StatusCode::SERVICE_UNAVAILABLE,
            retry_after: Some(RetryAfter::Delay(delay)),
        } => *delay == Duration::from_secs(1) || *delay == Duration::from_secs(5),
        LoadOutcome::Answered { status, .. } => landed.contains(status),
        LoadOutcome::Unanswered(_) => true,
    }
}

/// Kills the pooler's pool for `database` and resumes it after [`KILLED_FOR`].
///
/// # Errors
///
/// Returns [`PoolerConsoleError`] when the console refuses either command.
pub async fn kill_then_resume(console: &PoolerConsole, database: &DatabaseName) -> Result<(), PoolerConsoleError> {
    console.kill(database).await?;
    sleep(KILLED_FOR).await;
    console.resume(database).await
}

/// The most requests a drive through an outage sends: one per [`REQUEST_SPACING`] over [`RECOVERY_BOUND`], and a
/// margin.
pub const MAX_OUTAGE_REQUESTS: usize = 240;

/// Sends `request` for each of `items` in turn, every [`REQUEST_SPACING`], until an answer `served` holds for arrives
/// after the pool was resumed, [`RECOVERY_BOUND`] passes or the items run out, and answers each item sent with its
/// answer, in order.
pub async fn through_outage<I, T, F, Fut>(items: impl IntoIterator<Item = I>, mut request: F, served: impl Fn(&T) -> bool) -> Vec<(I, T)>
where
    F: FnMut(&I) -> Fut,
    Fut: Future<Output = T>,
{
    let started = Instant::now();
    let mut answered = Vec::new();
    for item in items {
        let answer = request(&item).await;
        let recovered = served(&answer) && started.elapsed() > KILLED_FOR * 2;
        answered.push((item, answer));
        if recovered || started.elapsed() > RECOVERY_BOUND {
            break;
        }
        sleep(REQUEST_SPACING).await;
    }
    answered
}
