//! The activity horizon of a Temporal Evolution, `evolution_active_until`:
//! the instant every write of the column rounds to.
//!
//! A bounded temporal window (clause 4.11) keeps an Evolution only for an
//! instance of a concerned attribute whose instant on the window's axis lies
//! inside it, so an Evolution none of whose instances reaches the window's
//! start on any axis can never match. The horizon bounds every such instant
//! from above and is the end of the UTC hour holding it. The history
//! application raises the horizon to cover every instant it records, and a
//! create that establishes the Evolution mints the row with the horizon its
//! history reaches, so the history leaves the indexed column alone and the
//! identity row keeps one version. A horizon above the instants the
//! Evolution holds only adds a candidate the window match then rejects.

use andromeda_postgresql_seaquery::{date_field::date_trunc, interval::make_interval_secs};
use sea_query::{Expr, ExprTrait, SimpleExpr};

/// The seconds of the hour a horizon rounds up to the end of.
const HOUR_SECONDS: f64 = 3_600.0;

/// The horizon covering `instant`: the end of the UTC hour holding it,
/// `date_trunc('hour', <instant>, 'UTC') + (make_interval(secs => 3600))`. An
/// instant on the hour gets the hour after it, which still bounds it, and a
/// NULL instant has no horizon.
#[must_use]
pub fn horizon_of(instant: SimpleExpr) -> SimpleExpr {
    date_trunc("hour", instant).add(make_interval_secs(Expr::val(HOUR_SECONDS)))
}

#[cfg(test)]
mod tests {
    use crate::evolution_horizon::horizon_of;
    use pretty_assertions::assert_eq;
    use sea_query::{Alias, Expr, PostgresQueryBuilder, Query};

    #[test]
    fn a_horizon_is_the_end_of_the_utc_hour_holding_its_instant() {
        let sql = Query::select()
            .expr(horizon_of(Expr::col(Alias::new("observed_at"))))
            .to_owned()
            .to_string(PostgresQueryBuilder);
        assert_eq!(sql, r#"SELECT date_trunc('hour', "observed_at", 'UTC') + (make_interval(secs => 3600))"#);
    }
}
