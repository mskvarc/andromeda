//! Verification of the name dictionaries' capacity and their unbounded growth.
//!
//! Both dictionary keys have `INTEGER` range, well past the 32,767-entry ceiling a `SMALLINT` key would impose by
//! accident. Neither dictionary is bounded above the database: only each
//! tenant's in-process cache of a dictionary is, by bytes, and a name the cache
//! does not hold is read from its table. The attribute dictionary is checked
//! for capacity, for growth past ten thousand names, and for resolution through
//! a cache far smaller than it; the entity type dictionary shares the
//! implementation and is checked for growth.
//!
//! Every test here changes the size of a shared relation, so each holds the
//! harness's exclusive lease for its duration. The dictionaries admit an insert
//! and nothing else, so a test never removes the names it minted: each run
//! mints under a prefix of its own and never meets an earlier run's names.
//!
//! Naming convention: `<subject>_<scenario>_<expected outcome>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

mod cache_eviction;
mod capacity;
mod entity_type_unbounded_growth;
mod unbounded_growth;

use andromeda_postgresql_seaquery::{
    aggregate::count_all,
    cast::cast,
    catalog_function::generate_series,
    pg_operator::PgBinaryOperator,
    pg_type::PostgresType,
};
use sea_query::{Alias, Expr, ExprTrait, InsertStatement, IntoColumnRef, IntoIden, IntoTableRef, OnConflict, Query, SelectStatement};
use uuid::Uuid;

/// The suite name every environment in this suite is allocated under.
pub const SUITE: &str = "name_dictionary";

/// The insert interning `count` distinct names, `prefix` followed by 1 to
/// `count`, into the dictionary `table`; a name already interned is left as it
/// is.
///
/// # Panics
///
/// Panics when the projection does not match the column list, which no
/// caller's arguments can cause.
pub fn generated_names_insert(table: impl IntoTableRef, name: impl IntoIden + Copy, prefix: &str, count: i32) -> InsertStatement {
    Query::insert()
        .into_table(table)
        .columns([name])
        .select_from(
            Query::select()
                .expr(Expr::val(prefix).concat_text(cast(Expr::col(Alias::new("generated")), PostgresType::Text)))
                .from_function(generate_series(Expr::val(1), Expr::val(count)), Alias::new("generated"))
                .to_owned(),
        )
        .expect("the projection matches the column list")
        .on_conflict(OnConflict::column(name).do_nothing().to_owned())
        .to_owned()
}

/// The number of names the dictionary `table` holds, or only those that match
/// the `LIKE` pattern `pattern`.
pub fn name_count(table: impl IntoTableRef, name: impl IntoColumnRef, pattern: Option<String>) -> SelectStatement {
    let mut count = Query::select().expr(count_all()).from(table).to_owned();
    if let Some(pattern) = pattern {
        count.and_where(Expr::col(name).like(pattern));
    }
    count
}

/// Base of every attribute or type IRI these tests mint.
const GENERATED_NAME_BASE: &str = "https://example.test/andromeda/dictionary-verification/";

/// A prefix beneath [`GENERATED_NAME_BASE`] that no other run mints under.
#[must_use]
pub fn run_name_prefix() -> String {
    format!("{GENERATED_NAME_BASE}{}/", Uuid::new_v4().simple())
}
