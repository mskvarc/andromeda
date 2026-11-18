//! Stopping the `TimescaleDB` background worker scheduler of one database.
//!
//! The extension's launcher keeps one scheduler session connected to every
//! database that allows connections and holds the extension. A database whose
//! scheduler is connected cannot be copied, so a database made a template has
//! its scheduler stopped once it refuses new connections.

use sea_query::{Func, Iden, Query, SelectStatement};

/// The `_timescaledb_functions.stop_background_workers()` function.
#[derive(Iden, Clone, Copy)]
#[iden = "_timescaledb_functions.stop_background_workers"]
struct StopBackgroundWorkers;

/// `SELECT _timescaledb_functions.stop_background_workers()`: end the
/// scheduler of the current database, which the launcher starts again only
/// when told to or when the server restarts.
#[must_use]
pub fn stop_background_workers() -> SelectStatement {
    Query::select().expr(Func::cust(StopBackgroundWorkers)).to_owned()
}

#[cfg(test)]
mod tests {
    use crate::timescale_scheduler::stop_background_workers;
    use pretty_assertions::assert_eq;
    use sea_query::PostgresQueryBuilder;

    #[test]
    fn the_scheduler_is_stopped_by_the_extensions_own_function() {
        assert_eq!(
            stop_background_workers().to_string(PostgresQueryBuilder),
            "SELECT _timescaledb_functions.stop_background_workers()"
        );
    }
}
