//! The entries of the claimed intents, read where they wait.
//!
//! Every statement of an application reads the same source: the claimed rows
//! of `temporal_append_intents`, each beside every entry it holds and that
//! entry's 1-based position in it. A statement adds the catalogue relations it
//! needs as further `FROM` items joined in its `WHERE`, because the position
//! comes from an `UNNEST` of the intent row, and a `JOIN ... ON` after it
//! could not name the intent.

use andromeda_postgresql_schema::{queue::TemporalAppendIntents, temporal_intent::TemporalEntry};
use andromeda_postgresql_seaquery::{
    pg_array::any,
    unnest::{Ordinality, apply_record_ordinality, unnest_of},
};
use sea_query::{Expr, ExprTrait, Iden, PostgresQueryBuilder, SelectStatement, SimpleExpr};
use sea_query_sqlx::{SqlxBinder, SqlxValues};

/// The `temporal_append_intents` alias, `i`.
#[derive(Iden, Clone, Copy)]
#[iden = "i"]
pub(crate) struct I;

/// The alias of one unnested entry, `e`.
#[derive(Iden, Clone, Copy)]
#[iden = "e"]
pub(crate) struct E;

/// The `temporal_entities` alias, `te`.
#[derive(Iden, Clone, Copy)]
#[iden = "te"]
pub(crate) struct Te;

/// The `temporal_attribute_streams` alias, `s`.
#[derive(Iden, Clone, Copy)]
#[iden = "s"]
pub(crate) struct S;

/// Read the claimed intents named by `keys` in `select`: the intent rows under
/// `i`.
pub(crate) fn claimed_intents(select: &mut SelectStatement, keys: &[i64]) {
    select
        .from_as(TemporalAppendIntents::Table, I)
        .and_where(Expr::col((I, TemporalAppendIntents::IntentKey)).eq(any(Expr::val(keys.to_vec()))));
}

/// Read every entry of the claimed intents named by `keys` in `select`: the
/// intent rows under `i`, each entry under `e`.
pub(crate) fn claimed_entries(select: &mut SelectStatement, keys: &[i64]) {
    claimed_intents(select, keys);
    select.from_function(unnest_of(Expr::col((I, TemporalAppendIntents::Entries))), E);
}

/// One field of the entry under `e`.
pub(crate) fn entry(field: TemporalEntry) -> SimpleExpr {
    Expr::col((E, field))
}

/// One column of the intent row under `i`.
pub(crate) fn intent(column: TemporalAppendIntents) -> SimpleExpr {
    Expr::col((I, column))
}

/// The write ordinal of the entry under `e`: its intent's base ordinal plus
/// its position, counted from 0.
pub(crate) fn write_ordinal() -> SimpleExpr {
    intent(TemporalAppendIntents::BaseOrdinal).add(Expr::col((E, Ordinality)).sub(1))
}

/// Whether the entry under `e` is one of `kinds`.
pub(crate) fn entry_kind_in(kinds: &[i16]) -> SimpleExpr {
    entry(TemporalEntry::EntryKind).is_in(kinds.iter().copied())
}

/// The rendered statement and its values, its entries unnested with their
/// positions.
pub(crate) fn rendered(statement: &impl SqlxBinder) -> (String, SqlxValues) {
    let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
    (apply_record_ordinality(sql, &E), values)
}
