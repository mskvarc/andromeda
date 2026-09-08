//! Verification-matrix row "Physical column layout".
//!
//! The three entity relations, the projection and the temporal intent queue
//! declare their columns in alignment order: not-null fixed-width first and widest-first, then nullable
//! fixed-width, then varlena.
//! The catalog states the physical order, and a naive declaration order costs
//! measurable padding for the same data.

use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use sqlx::{Executor, FromRow, PgPool};

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
    sqlx::query_as(
        "SELECT a.attname, a.attlen, a.attalign::text, a.attnotnull FROM pg_attribute AS a \
         JOIN pg_class AS c ON c.oid = a.attrelid \
         WHERE c.relname = $1 AND a.attnum > 0 AND NOT a.attisdropped ORDER BY a.attnum",
    )
    .bind(relation)
    .fetch_all(pool)
    .await
    .expect("the relation must exist")
}

#[tokio::test]
async fn rebuilt_relations_declare_a_not_null_fixed_prefix_without_alignment_padding() {
    let scope = TestEnvironment::from_suite("query-access-paths").await.expect("isolated test environment");
    let pool = scope.database_pool();

    for relation in [
        "entities",
        "entity_documents",
        "entity_search",
        "current_attribute_values",
        "temporal_append_intents",
    ] {
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
    let scope = TestEnvironment::from_suite("query-access-paths").await.expect("isolated test environment");
    let mut transaction = scope.database_pool().begin().await.expect("transaction must start");

    // The same five values twice: once in the projection's alignment order,
    // once in the order a schema author would naturally type them.
    for statement in [
        "CREATE TEMPORARY TABLE layout_aligned (\
             entity_key BIGINT NOT NULL, modified_at TIMESTAMPTZ NOT NULL, attribute_name_id INTEGER NOT NULL, \
             attribute_kind SMALLINT NOT NULL, value_kind SMALLINT NOT NULL, value_bool BOOLEAN, value_string TEXT) ON COMMIT DROP",
        "CREATE TEMPORARY TABLE layout_naive (\
             attribute_kind SMALLINT NOT NULL, entity_key BIGINT NOT NULL, value_bool BOOLEAN, \
             attribute_name_id INTEGER NOT NULL, modified_at TIMESTAMPTZ NOT NULL, value_string TEXT, \
             value_kind SMALLINT NOT NULL) ON COMMIT DROP",
        "INSERT INTO layout_aligned SELECT g, now(), g % 1000, 1, 2, true, 'value-' || g::text FROM generate_series(1, 20000) AS g",
        "INSERT INTO layout_naive SELECT 1, g, true, g % 1000, now(), 'value-' || g::text, 2 FROM generate_series(1, 20000) AS g",
    ] {
        transaction.execute(statement).await.expect("fixture statement must succeed");
    }

    let aligned: i64 = sqlx::query_scalar("SELECT sum(pg_column_size(t.*))::bigint FROM layout_aligned AS t")
        .fetch_one(&mut *transaction)
        .await
        .expect("aligned width must be readable");
    let naive: i64 = sqlx::query_scalar("SELECT sum(pg_column_size(t.*))::bigint FROM layout_naive AS t")
        .fetch_one(&mut *transaction)
        .await
        .expect("naive width must be readable");
    assert!(
        aligned <= naive,
        "alignment order must not be wider than declaration order: aligned={aligned}, naive={naive}"
    );

    transaction.rollback().await.expect("rollback must succeed");

    scope.finish().await.expect("cleanup failed");
}
