//! The keys a density-measured GIN holds per covered row, and the bytes one key
//! takes in it.
//!
//! A GIN is its keys and the posting entries that point each key at its rows,
//! so its size is its covered rows times its keys per row times the bytes one
//! key takes. The keys per row follow the data and change with it; the bytes
//! per key follow the shape of the index. Rebuilt trigram GINs over entity
//! identifiers measure 1.5 to 1.8 bytes per trigram, and rebuilt trigram GINs
//! over an attribute name and a text 1.3, whatever identifiers or texts they
//! hold. A GIN is therefore judged against the size its covered rows take at
//! the keys per row its table holds now and at the bytes per key a rebuild of
//! it measures, and its degradation is its size above that.
//!
//! The keys per row are sampled from the GIN's table each time its analyzed
//! row count is recorded, so values that yield more keys raise the size a GIN
//! is judged against instead of reading as degradation. The bytes per key are
//! measured on the GIN after each rebuild. Before its first rebuild a GIN takes
//! the default of its shape, set at the low end of what rebuilt GINs of that
//! shape measure: a default below the truth overstates degradation by a few
//! points and costs one rebuild that then measures the GIN, while a default
//! above it would understate degradation and never let the rebuild happen. A
//! GIN of a shape with no measured default (`jsonb_path_ops` and `bigint[]`
//! keys, whose size follows how many distinct keys they hold) measures its
//! bytes per key on its first observation instead, so degradation it carried
//! then reads as none until its first rebuild.

use crate::{branch::Branch, candidate::index_target::DensityColumn, error::MaintenanceError};
use andromeda_postgresql_seaquery::{
    cast::cast_compound,
    jsonb::jsonb_path_query,
    pg_array::cardinality,
    pg_type::PostgresType,
    scalar_subquery::scalar_subquery,
    statement::Statement,
    trigram::show_trgm,
};
use sea_query::{
    Alias,
    Asterisk,
    Expr,
    ExprTrait,
    Func,
    Iden,
    Query,
    SelectStatement,
    SimpleExpr,
    extension::postgres::{PostgresSelectStatementExt, SampleMethod},
};
use sqlx::PgConnection;
use std::num::NonZeroU64;

/// Keys per row and bytes per key are held in thousandths, so a value of a few
/// units keeps three significant digits without a float.
const THOUSANDTHS: u32 = 1_000;

/// Covered rows a keys-per-row sample aims to read: enough for the mean to
/// hold within a fraction of a key.
const SAMPLE_ROWS: u64 = 10_000;

/// Heap pages a keys-per-row sample reads at most, so a sample of a column
/// few rows hold never reads its whole table.
const SAMPLE_PAGES: u64 = 1_024;

/// The `jsonb_path_ops` keys of a `jsonb` value: one per scalar it holds.
const JSONB_SCALARS: &str = r#"strict $.** ? (@.type() != "object" && @.type() != "array")"#;

/// The column a keys-per-row sample projects.
#[derive(Clone, Copy, Iden)]
enum KeySampleColumn {
    /// Thousandths of a key per sampled row, NULL when the sample read none.
    KeysPerRow,
}

/// A GIN's keys per covered row, in thousandths of a key, never zero.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct KeysPerRow(NonZeroU64);

impl KeysPerRow {
    /// Keys per row from thousandths of a key.
    pub(crate) const fn from_thousandths(thousandths: NonZeroU64) -> KeysPerRow {
        KeysPerRow(thousandths)
    }

    /// Thousandths of a key per covered row.
    pub(crate) const fn thousandths(self) -> u64 {
        self.0.get()
    }
}

/// The bytes one key takes in a GIN, in thousandths of a byte, never zero.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct KeyBytes(NonZeroU64);

impl KeyBytes {
    /// Bytes per key from thousandths of a byte.
    pub(crate) const fn from_thousandths(thousandths: NonZeroU64) -> KeyBytes {
        KeyBytes(thousandths)
    }

    /// Thousandths of a byte per key.
    pub(crate) const fn thousandths(self) -> u64 {
        self.0.get()
    }
}

/// The bytes per key a GIN over `column` is judged at before its first
/// rebuild measures its own, `None` for a shape with no measured default.
pub(crate) const fn default_key_bytes(column: DensityColumn) -> Option<KeyBytes> {
    match column {
        DensityColumn::Id => Some(KeyBytes(NonZeroU64::MIN.saturating_add(1_499))),
        DensityColumn::ValueLongText => Some(KeyBytes(NonZeroU64::MIN.saturating_add(1_299))),
        DensityColumn::SubEqKeys | DensityColumn::ValueObject => None,
    }
}

/// The keys the GIN over `column` holds for one row: its trigrams, one more
/// for the attribute name key a two-column GIN leads with, and one for a value
/// that yields none, which a GIN keeps as an empty item.
fn row_keys(column: DensityColumn) -> SimpleExpr {
    let value = Expr::col(Alias::new(<&'static str>::from(column)));
    match column {
        DensityColumn::Id => cardinality(show_trgm(value)),
        DensityColumn::ValueLongText => Expr::val(1).add(cardinality(show_trgm(value))),
        DensityColumn::SubEqKeys => Func::greatest([cardinality(value), Expr::val(1)]).into(),
        DensityColumn::ValueObject => {
            let scalars = Query::select()
                .expr(Func::count(Expr::col(Asterisk)))
                .from_function(jsonb_path_query(value, JSONB_SCALARS), Alias::new("k"))
                .to_owned();
            Expr::val(1).add(Func::greatest([scalar_subquery(scalars), Expr::val(1)]))
        }
    }
}

/// How many times a sample halves the heap's pages to meet about
/// [`SAMPLE_ROWS`] of `covered_rows` without passing [`SAMPLE_PAGES`] of
/// `heap_pages`. Halvings keep the statement text to one of a few forms.
fn sample_halvings(covered_rows: u64, heap_pages: u64) -> u32 {
    let fraction = Ord::max(Ord::max(covered_rows.div_ceil(SAMPLE_ROWS), heap_pages.div_ceil(SAMPLE_PAGES)), 1);
    fraction.next_power_of_two().trailing_zeros()
}

/// The percentage of the heap's pages `halvings` halvings of it leave.
fn sample_percentage(halvings: u32) -> f64 {
    (0..halvings).fold(100.0, |percentage, _| percentage / 2.0)
}

/// The mean keys per row of `column` over a `SYSTEM` sample of `percentage` of
/// its heap's pages in `branch`, in thousandths of a key.
fn keys_per_row_statement(branch: Branch, column: DensityColumn, percentage: f64) -> SelectStatement {
    let keys = SimpleExpr::from(Func::avg(row_keys(column))).mul(Expr::val(i64::from(THOUSANDTHS)));
    Query::select()
        .expr_as(cast_compound(keys, PostgresType::Bigint), KeySampleColumn::KeysPerRow)
        .from((branch.schema(), Alias::new(column.gin().heap().relation_name())))
        .table_sample(SampleMethod::SYSTEM, percentage, None)
        .and_where(Expr::col(Alias::new(<&'static str>::from(column))).is_not_null())
        .to_owned()
}

/// Sample the keys per row the GIN over `column` holds in `branch`, from a
/// table of `heap_pages` pages whose `covered_rows` rows it covers. `None` when
/// the sample read no covered row.
pub(crate) async fn sample_keys_per_row(
    connection: &mut PgConnection,
    branch: Branch,
    column: DensityColumn,
    covered_rows: u64,
    heap_pages: u64,
) -> Result<Option<KeysPerRow>, MaintenanceError> {
    let statement = keys_per_row_statement(branch, column, sample_percentage(sample_halvings(covered_rows, heap_pages)));
    let thousandths = statement.to_query_scalar::<Option<i64>>().fetch_one(connection).await?;
    Ok(thousandths
        .and_then(|value| u64::try_from(value).ok())
        .and_then(NonZeroU64::new)
        .map(KeysPerRow))
}

#[cfg(test)]
mod tests {
    use crate::{
        branch::Branch,
        candidate::index_target::DensityColumn,
        gin_keys::{KeyBytes, default_key_bytes, keys_per_row_statement, row_keys, sample_halvings, sample_percentage},
    };
    use pretty_assertions::assert_eq;
    use sea_query::{PostgresQueryBuilder, Query};

    fn rendered_keys(column: DensityColumn) -> String {
        Query::select().expr(row_keys(column)).to_owned().to_string(PostgresQueryBuilder)
    }

    #[test]
    fn a_trigram_gin_over_the_identifiers_holds_the_trigrams_of_each_identifier() {
        assert_eq!(rendered_keys(DensityColumn::Id), r#"SELECT cardinality(show_trgm("id"))"#);
    }

    #[test]
    fn a_gin_leading_with_the_attribute_name_key_holds_one_key_more_per_row() {
        assert_eq!(
            rendered_keys(DensityColumn::ValueLongText),
            r#"SELECT 1 + cardinality(show_trgm("value_long_text"))"#
        );
        assert_eq!(
            rendered_keys(DensityColumn::ValueObject),
            r#"SELECT 1 + GREATEST((SELECT COUNT(*) FROM jsonb_path_query("value_object", 'strict $.** ? (@.type() != "object" && @.type() != "array")') AS "k"), 1)"#
        );
    }

    #[test]
    fn an_array_gin_holds_one_key_per_element_and_an_empty_item_for_an_empty_array() {
        assert_eq!(rendered_keys(DensityColumn::SubEqKeys), r#"SELECT GREATEST(cardinality("sub_eq_keys"), 1)"#);
    }

    /// 10,000 of 2,047,710 covered rows are a 205th of the table and 1,024 of
    /// 99,013 pages a 97th: the smaller share, a 205th, rounds down to a 256th,
    /// eight halvings, 0.390625 % of the pages, 387 of them.
    #[test]
    fn a_sample_reads_about_ten_thousand_covered_rows_in_halvings_of_the_table() {
        assert_eq!(sample_halvings(2_047_710, 99_013), 8);
        assert_eq!(sample_halvings(20_000, 300), 1);
        assert_eq!(sample_halvings(5_000, 50), 0);
        assert_eq!(sample_halvings(0, 0), 0);
    }

    /// 10,000 of 60,000 covered rows would take a sixth of a 200,000-page
    /// table; 1,024 pages are a 196th of it, which rounds down to a 256th,
    /// eight halvings.
    #[test]
    fn a_sample_of_a_column_few_rows_hold_reads_at_most_a_thousand_pages() {
        assert_eq!(sample_halvings(60_000, 200_000), 8);
    }

    #[test]
    fn the_sample_reads_the_branch_heap_rows_holding_the_column() {
        let sql = keys_per_row_statement(Branch::Live, DensityColumn::ValueLongText, sample_percentage(8)).to_string(PostgresQueryBuilder);
        assert_eq!(
            sql,
            r#"SELECT (AVG(1 + cardinality(show_trgm("value_long_text"))) * 1000)::bigint AS "keys_per_row" FROM "public"."current_attributes" TABLESAMPLE SYSTEM (0.390625) WHERE "value_long_text" IS NOT NULL"#
        );
    }

    #[test]
    fn only_trigram_gins_have_a_default_bytes_per_key() {
        assert_eq!(default_key_bytes(DensityColumn::Id).map(KeyBytes::thousandths), Some(1_500));
        assert_eq!(default_key_bytes(DensityColumn::ValueLongText).map(KeyBytes::thousandths), Some(1_300));
        assert_eq!(default_key_bytes(DensityColumn::SubEqKeys), None);
        assert_eq!(default_key_bytes(DensityColumn::ValueObject), None);
    }
}
