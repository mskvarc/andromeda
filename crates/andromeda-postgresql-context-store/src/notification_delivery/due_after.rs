//! The instant a retried delivery, or a held Subscription's next probe, falls
//! due.
//!
//! A claim compares every due instant with `CURRENT_TIMESTAMP`, so a statement
//! that reschedules a delivery sets the same clock plus the worker's wait. An
//! instant computed on the broker's clock would put the retry early or late by
//! however far that clock is from the store's. `CURRENT_TIMESTAMP` is the
//! instant the recording transaction began, so the wait runs from no earlier
//! than the end of the attempt it follows.

use andromeda_notification::delivery_backoff::BackoffStep;
use andromeda_postgresql_seaquery::interval::make_interval_secs;
use sea_query::{Expr, ExprTrait, SimpleExpr};

/// `CURRENT_TIMESTAMP + make_interval(secs => <seconds>)`: due `seconds`
/// after the transaction began.
pub(crate) fn due_after(seconds: SimpleExpr) -> SimpleExpr {
    Expr::current_timestamp().add(make_interval_secs(seconds))
}

/// The bound seconds of `step`.
pub(crate) fn step_seconds(step: BackoffStep) -> SimpleExpr {
    Expr::val(step.seconds())
}

#[cfg(test)]
mod tests {
    use crate::notification_delivery::due_after::{due_after, step_seconds};
    use andromeda_notification::delivery_backoff::DeliveryBackoff;
    use andromeda_retry::schedule::BackoffSchedule;
    use sea_query::{PostgresQueryBuilder, Query};
    use std::time::Duration;

    #[test]
    fn a_wait_is_added_to_the_stores_clock_as_bound_seconds() {
        let backoff = DeliveryBackoff::new(BackoffSchedule::constant(Duration::from_millis(1_500))).expect("a schedule within the longest step");
        let (sql, values) = Query::select().expr(due_after(step_seconds(backoff.first_step()))).build(PostgresQueryBuilder);
        pretty_assertions::assert_eq!(sql, "SELECT CURRENT_TIMESTAMP + (make_interval(secs => $1))");
        pretty_assertions::assert_eq!(values.0, vec![1.5_f64.into()]);
    }
}
