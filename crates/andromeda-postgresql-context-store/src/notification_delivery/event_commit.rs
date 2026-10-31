//! Which commit a stored notification event records the instant of.

use andromeda_context_commit::store_instant::StoreInstant;
use andromeda_postgresql_seaquery::clock::now;
use sea_query::{Expr, SimpleExpr};

/// The commit that made the change a notification event describes.
///
/// A delivery compares that commit's instant with the instant its
/// Subscription last became active, both read from the store's clock, so a
/// change committed while the Subscription was paused is never delivered once
/// it is active again (clause 5.8.6).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EventCommit {
    /// The transaction inserting the event is the one that made the change.
    ThisTransaction,
    /// A replay queues the event of a change an earlier commit made, at this
    /// instant.
    Earlier(StoreInstant),
}

impl EventCommit {
    /// The value the `committed_at` column is written with.
    pub(crate) fn value(self) -> SimpleExpr {
        match self {
            EventCommit::ThisTransaction => now(),
            EventCommit::Earlier(at) => Expr::val(at.get()),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::notification_delivery::event_commit::EventCommit;
    use andromeda_context_commit::store_instant::StoreInstant;
    use chrono::{DateTime, Utc};
    use sea_query::{PostgresQueryBuilder, Query};

    fn rendered(commit: EventCommit) -> String {
        Query::select().expr(commit.value()).to_string(PostgresQueryBuilder)
    }

    #[test]
    fn an_event_of_this_transaction_takes_its_transaction_timestamp() {
        pretty_assertions::assert_eq!(rendered(EventCommit::ThisTransaction), "SELECT NOW()");
    }

    #[test]
    fn an_event_of_an_earlier_commit_keeps_that_commits_instant() {
        let at = DateTime::<Utc>::UNIX_EPOCH;
        assert!(rendered(EventCommit::Earlier(StoreInstant::new(at))).contains("1970-01-01 00:00:00"));
    }
}
