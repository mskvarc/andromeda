//! Physical column layout: columns are declared in alignment order.
//!
//! The three entity relations, the projection and the temporal intent queue
//! declare their columns in alignment order: not-null fixed-width first and widest-first, then nullable
//! fixed-width, then varlena.
//! The catalog states the physical order, and a naive declaration order costs
//! measurable padding for the same data.

use andromeda_postgresql_seaquery::{
    cast::cast,
    catalog_function::{column_size, generate_series},
    clock::now,
    command::Command,
    literal::{text_literal, true_literal},
    pg_operator::PgBinaryOperator,
    pg_type::PostgresType,
    statement::Statement,
    system_catalog::PgClass,
    temporary_table::{ColumnConstraint, TemporaryColumn, TemporaryTable},
};
use andromeda_test_harness::serialized_environment::SerializedEnvironment;
use pretty_assertions::assert_eq;
use sea_query::{Alias, Asterisk, Expr, ExprTrait, Func, InsertStatement, Order, Query, SimpleExpr};
use sqlx::{FromRow, PgConnection, PgPool};

/// One column of a relation as the catalog describes it physically.
#[derive(Debug, FromRow)]
struct PhysicalColumn {
    /// Column name.
    attname: String,
    /// Declared width in bytes, negative for varlena.
    attlen: i16,
    /// Alignment class: `c`, `s`, `i` or `d`.
    attalign: String,
    /// Whether the column is declared `NOT NULL`.
    attnotnull: bool,
}

impl PhysicalColumn {
    /// Alignment boundary in bytes implied by the alignment class, or `None`
    /// when `pg_attribute` reports a class outside the four PostgreSQL defines.
    fn alignment(&self) -> Option<usize> {
        match self.attalign.as_str() {
            "c" => Some(1),
            "s" => Some(2),
            "i" => Some(4),
            "d" => Some(8),
            _ => None,
        }
    }

    /// Whether the column is a fixed-width, non-null-bearing prefix candidate.
    const fn is_fixed_width(&self) -> bool {
        self.attlen > 0
    }
}

/// Read the physical column order of one relation.
async fn physical_columns(pool: &PgPool, relation: &str) -> Vec<PhysicalColumn> {
    let attribute = |name: &str| Expr::col((Alias::new("pg_attribute"), Alias::new(name)));
    Query::select()
        .columns([Alias::new("attname"), Alias::new("attlen")].map(|column| (Alias::new("pg_attribute"), column)))
        .expr(cast(attribute("attalign"), PostgresType::Text))
        .column((Alias::new("pg_attribute"), Alias::new("attnotnull")))
        .from(Alias::new("pg_attribute"))
        .inner_join(
            PgClass::Table,
            Expr::col((PgClass::Table, PgClass::Oid)).equals((Alias::new("pg_attribute"), Alias::new("attrelid"))),
        )
        .and_where(Expr::col((PgClass::Table, PgClass::Relname)).eq(relation))
        .and_where(attribute("attnum").gt(0))
        .and_where(attribute("attisdropped").not())
        .order_by((Alias::new("pg_attribute"), Alias::new("attnum")), Order::Asc)
        .to_query_as()
        .fetch_all(pool)
        .await
        .expect("the relation must exist")
}

/// The width of every row of the fixture relation `table`.
async fn total_row_width(transaction: &mut PgConnection, table: &str) -> i64 {
    Query::select()
        .expr(cast(
            Func::sum(column_size(Expr::col((Alias::new("t"), Asterisk)))).into(),
            PostgresType::Bigint,
        ))
        .from_as(Alias::new(table), Alias::new("t"))
        .to_query_scalar()
        .fetch_one(transaction)
        .await
        .expect("row width must be readable")
}

/// The insert of 20000 rows into the fixture relation `table`, `values` the
/// cells of one row in the relation's column order over `g`.
fn fixture_rows(table: &str, columns: &[&str], values: Vec<SimpleExpr>) -> InsertStatement {
    Query::insert()
        .into_table(Alias::new(table))
        .columns(columns.iter().map(|column| Alias::new(*column)))
        .select_from(
            Query::select()
                .exprs(values)
                .from_function(generate_series(Expr::val(1), Expr::val(20_000)), Alias::new("g"))
                .to_owned(),
        )
        .expect("the projection matches the column list")
        .to_owned()
}

#[tokio::test]
async fn rebuilt_relations_declare_a_not_null_fixed_prefix_without_alignment_padding() {
    let scope = SerializedEnvironment::from_environment_reset().await.expect("serialized test environment");
    let pool = scope.database_pool();

    for relation in ["entities", "entity_search", "current_attributes", "temporal_append_intents"] {
        let columns = physical_columns(pool, relation).await;
        let prefix: Vec<&PhysicalColumn> = columns.iter().take_while(|column| column.attnotnull && column.is_fixed_width()).collect();
        assert!(!prefix.is_empty(), "{relation} must open with a not-null fixed-width prefix");

        let mut offset = 0_usize;
        for column in &prefix {
            let alignment = column.alignment().expect("pg_attribute reports one of the four alignment classes");
            assert_eq!(
                offset % alignment,
                0,
                "{relation}.{} starts at offset {offset}, which needs {alignment}-byte padding",
                column.attname
            );
            offset += usize::try_from(column.attlen).expect("fixed widths are positive");
        }

        // Nothing fixed-width and not-null may hide behind a nullable or
        // varlena column, where it would pay for the alignment of whatever
        // precedes it.
        let tail_violation = columns.iter().skip(prefix.len()).find(|column| column.attnotnull && column.is_fixed_width());
        assert!(
            tail_violation.is_none(),
            "{relation} declares {:?} after the varying part of the tuple",
            tail_violation.map(|column| &column.attname)
        );
    }

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn alignment_order_is_no_wider_than_a_naive_declaration_order() {
    let scope = SerializedEnvironment::from_environment_reset().await.expect("serialized test environment");
    let mut transaction = scope.database_pool().begin().await.expect("transaction must start");

    // The same five values twice: once in the projection's alignment order,
    // once in the order a schema author would naturally type them.
    let column = |name: &str, ty: PostgresType, constraint: ColumnConstraint| TemporaryColumn::new(Alias::new(name), ty, constraint);
    let series = || Expr::col(Alias::new("g"));
    let value_text = || text_literal("value-").concat_text(cast(series(), PostgresType::Text));
    let aligned_columns = [
        "entity_key",
        "modified_at",
        "attribute_name_key",
        "attribute_kind",
        "value_kind",
        "value_boolean",
        "value_string",
    ];
    let naive_columns = [
        "attribute_kind",
        "entity_key",
        "value_boolean",
        "attribute_name_key",
        "modified_at",
        "value_string",
        "value_kind",
    ];
    TemporaryTable::new(
        Alias::new("layout_aligned"),
        vec![
            column("entity_key", PostgresType::Bigint, ColumnConstraint::NotNull),
            column("modified_at", PostgresType::Timestamptz, ColumnConstraint::NotNull),
            column("attribute_name_key", PostgresType::Integer, ColumnConstraint::NotNull),
            column("attribute_kind", PostgresType::Smallint, ColumnConstraint::NotNull),
            column("value_kind", PostgresType::Smallint, ColumnConstraint::NotNull),
            column("value_boolean", PostgresType::Boolean, ColumnConstraint::Nullable),
            column("value_string", PostgresType::Text, ColumnConstraint::Nullable),
        ],
    )
    .to_query()
    .execute(&mut *transaction)
    .await
    .expect("fixture statement must succeed");
    TemporaryTable::new(
        Alias::new("layout_naive"),
        vec![
            column("attribute_kind", PostgresType::Smallint, ColumnConstraint::NotNull),
            column("entity_key", PostgresType::Bigint, ColumnConstraint::NotNull),
            column("value_boolean", PostgresType::Boolean, ColumnConstraint::Nullable),
            column("attribute_name_key", PostgresType::Integer, ColumnConstraint::NotNull),
            column("modified_at", PostgresType::Timestamptz, ColumnConstraint::NotNull),
            column("value_string", PostgresType::Text, ColumnConstraint::Nullable),
            column("value_kind", PostgresType::Smallint, ColumnConstraint::NotNull),
        ],
    )
    .to_query()
    .execute(&mut *transaction)
    .await
    .expect("fixture statement must succeed");
    fixture_rows(
        "layout_aligned",
        &aligned_columns,
        vec![
            series(),
            now(),
            Expr::expr(series().modulo(1000)),
            cast(Expr::val(1), PostgresType::Smallint),
            cast(Expr::val(2), PostgresType::Smallint),
            true_literal(),
            value_text(),
        ],
    )
    .to_query()
    .execute(&mut *transaction)
    .await
    .expect("fixture statement must succeed");
    fixture_rows(
        "layout_naive",
        &naive_columns,
        vec![
            cast(Expr::val(1), PostgresType::Smallint),
            series(),
            true_literal(),
            Expr::expr(series().modulo(1000)),
            now(),
            value_text(),
            cast(Expr::val(2), PostgresType::Smallint),
        ],
    )
    .to_query()
    .execute(&mut *transaction)
    .await
    .expect("fixture statement must succeed");

    let aligned = total_row_width(&mut transaction, "layout_aligned").await;
    let naive = total_row_width(&mut transaction, "layout_naive").await;
    assert!(
        aligned <= naive,
        "alignment order must not be wider than declaration order: aligned={aligned}, naive={naive}"
    );

    transaction.rollback().await.expect("rollback must succeed");

    scope.finish().await.expect("cleanup failed");
}
