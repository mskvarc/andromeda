//! The lock of a tenant database's context source change clock that every registration write and every context
//! source subscription write of that tenant waits on, as a [`crate::row_hold::RowHold`] takes it.

use andromeda_postgresql_schema::context_source_change_clock::ContextSourceChangeClock;
use sea_query::{LockType, Query, SelectStatement};

/// The lock of the clock's one row that every advance of the clock waits on.
#[must_use]
pub fn change_clock_lock() -> SelectStatement {
    Query::select()
        .column(ContextSourceChangeClock::Revision)
        .from(ContextSourceChangeClock::Table)
        .lock(LockType::Update)
        .to_owned()
}

#[cfg(test)]
mod tests {
    use crate::change_clock_lock::change_clock_lock;
    use googletest::prelude::*;
    use sea_query::PostgresQueryBuilder;

    #[gtest]
    fn the_hold_locks_the_one_row_of_the_change_clock_for_update() -> Result<()> {
        verify_that!(
            change_clock_lock().to_string(PostgresQueryBuilder),
            eq(r#"SELECT "revision" FROM "context_source_change_clock" FOR UPDATE"#)
        )
    }
}
