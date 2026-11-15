//! The instant a statement judges clause 4.22 expiry at, as PostgreSQL reads
//! it.
//!
//! A request reads at the transaction's `NOW()`. An evaluation that fixed its
//! instant binds it as a `timestamptz`, so every statement it runs, and the
//! evaluation it hands the rows to in memory, judge expiry at the same instant
//! on the same clock.

use andromeda_ngsi_ld_query::standing_instant::StandingInstant;
use andromeda_postgresql_seaquery::clock::now;
use sea_query::{Expr, SimpleExpr};

/// `NOW()` for [`StandingInstant::WhenRead`], the bound instant for
/// [`StandingInstant::At`].
#[must_use]
pub fn instant_of(standing: StandingInstant) -> SimpleExpr {
    match standing {
        StandingInstant::WhenRead => now(),
        StandingInstant::At(at) => Expr::val(at),
    }
}

#[cfg(test)]
mod tests {
    use crate::standing_instant::instant_of;
    use andromeda_ngsi_ld_query::standing_instant::StandingInstant;
    use andromeda_postgresql_seaquery::assert_statement_snapshot;
    use andromeda_postgresql_stored_form::expiry::unexpired;
    use chrono::{DateTime, Utc};
    use sea_query::{Expr, Query};

    #[test]
    fn a_read_at_its_own_instant_reads_the_transaction_clock() {
        let statement = Query::select()
            .expr(unexpired(Expr::col(("ca", "expires_at")), instant_of(StandingInstant::WhenRead)))
            .to_owned();
        assert_statement_snapshot!("standing-instant-a-read-at-its-own-instant-reads-the-transaction-clock", statement);
    }

    /// An evaluation that fixed its instant binds it, so the statement reads
    /// no database clock.
    #[test]
    fn a_read_at_a_fixed_instant_binds_it() {
        let at = DateTime::<Utc>::from_timestamp(1_893_456_000, 0).expect("a valid instant");
        let statement = Query::select()
            .expr(unexpired(Expr::col(("ca", "expires_at")), instant_of(StandingInstant::At(at))))
            .to_owned();
        assert_statement_snapshot!("standing-instant-a-read-at-a-fixed-instant-binds-it", statement);
    }
}
