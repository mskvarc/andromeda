//! The statements a name cache reads and mints its dictionary with.
//!
//! Every statement names its relation and columns through the
//! [`DictionaryRelation`] it is generic over, so the attribute and entity type
//! dictionaries share one set of statements.

use crate::dictionary_relation::DictionaryRelation;
use andromeda_postgresql_seaquery::{
    introspection::transaction_has_not_written,
    pg_array::{is_any_of, text_array},
    pg_type::PostgresType,
    unnest::{UnnestColumn, unnest_call},
};
use sea_query::{Alias, Expr, InsertStatement, OnConflict, Order, Query, SelectStatement, error::Error as SeaQueryError};

/// The column that reports whether the transaction reading the dictionary has
/// written anything yet.
pub(crate) const SNAPSHOT_IS_COMMITTED_COLUMN: &str = "snapshot_is_committed";

/// At most `rows` mappings of the dictionary, for warming the cache.
///
/// Oldest key first: the names a tenant minted first are its core vocabulary,
/// the ones nearly every request resolves, so they are the ones worth holding
/// when the dictionary is larger than the cache.
pub(crate) fn preload_query<N: DictionaryRelation>(rows: u64) -> SelectStatement {
    Query::select()
        .columns([N::key_column(), N::name_column()])
        .from(N::table())
        .order_by(N::key_column(), Order::Asc)
        .limit(rows)
        .to_owned()
}

/// The read a minting resolution starts with: the keys of the names the
/// dictionary already holds, plus one flag telling whether the caller's
/// transaction holds a transaction id.
///
/// `pg_current_xact_id_if_assigned()` is null until the transaction's first
/// write, and a transaction that has written nothing cannot have inserted an
/// uncommitted dictionary row, so when the flag is true every row this read
/// returns is committed and may be published to the cache. Once the
/// transaction has written, the flag is false and the rows are withheld,
/// because one of them may be this transaction's own.
pub(crate) fn held_select_query<N: DictionaryRelation>(names: &[&str]) -> SelectStatement {
    let mut query = name_select_query::<N>(names);
    query.expr_as(transaction_has_not_written(), Alias::new(SNAPSHOT_IS_COMMITTED_COLUMN));
    query
}

/// Read back the keys of a batch of names, bound as one array so that a large
/// batch cannot exhaust the protocol's parameter space.
pub(crate) fn name_select_query<N: DictionaryRelation>(names: &[&str]) -> SelectStatement {
    Query::select()
        .columns([N::key_column(), N::name_column()])
        .from(N::table())
        .and_where(is_any_of(Expr::col(N::name_column()), names.iter().map(|name| (*name).to_owned())))
        .to_owned()
}

/// Read back the names of a batch of keys, bound as one array for the reason
/// [`name_select_query`] binds its names as one.
pub(crate) fn key_select_query<N: DictionaryRelation>(keys: &[i32]) -> SelectStatement {
    Query::select()
        .columns([N::key_column(), N::name_column()])
        .from(N::table())
        .and_where(is_any_of(Expr::col(N::key_column()), keys.iter().copied()))
        .to_owned()
}

/// Mint every name of a batch that the dictionary does not already hold.
///
/// One statement per batch rather than one per name, and `DO NOTHING` rather
/// than a prior existence check: another transaction may have minted the same
/// name between this one's read and its insert. The batch arrives as one bound
/// array through `UNNEST`, so a large batch cannot exhaust the parameter space.
pub(crate) fn name_insert_query<N: DictionaryRelation>(names: &[&str]) -> Result<InsertStatement, SeaQueryError> {
    let column = UnnestColumn::new(N::name_column(), PostgresType::TextArray);
    let source_alias = Alias::new("t");
    let mut source = Query::select();
    source
        .column(source_alias.clone())
        .from_function(unnest_call([(&column, text_array(names))]), source_alias);
    let mut insert = Query::insert();
    insert
        .into_table(N::table())
        .columns([N::name_column()])
        .select_from(source.clone())?
        .on_conflict(OnConflict::column(N::name_column()).do_nothing().to_owned());
    Ok(insert.clone())
}
