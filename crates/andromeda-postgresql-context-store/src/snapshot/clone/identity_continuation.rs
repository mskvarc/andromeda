//! Continuing a clone's identity sequences from where its source left off.
//!
//! A clone copies every identity value verbatim through `OVERRIDING SYSTEM
//! VALUE`, and PostgreSQL never advances an identity sequence for a value it
//! was given. The target branch is built fresh by the branch migrations, so its
//! sequences would still start at 1 and the first key a write to the clone mints
//! would collide with a copied row. Each identity sequence of the target is
//! therefore set, in the clone's copy transaction, to the larger of two
//! positions: the source sequence's, so the clone never mints again a key its
//! source already handed out and freed, and the largest value copied, so a
//! source whose sequence lags its own rows cannot pass that lag on. When both
//! are absent (an empty relation whose source never minted) the sequence is
//! left as the migrations made it.
//!
//! The source position is read from the source column's own sequence alone,
//! so a concurrent drop of another Snapshot's schema, whose sequences vanish
//! mid-statement, cannot fail the clone.

use crate::error::ContextStoreError;
use andromeda_postgresql_seaquery::{
    scalar_subquery::scalar_subquery,
    sequence::{identity_sequence, sequence_last_value, set_sequence_position},
    statement::Statement,
};
use sea_query::{DynIden, Expr, ExprTrait, Func, Iden, Query, SelectStatement, TableName};
use sqlx::PgConnection;

/// The derived table holding the position a sequence continues from.
#[derive(Iden, Clone, Copy)]
enum Continuation {
    /// The derived table.
    #[iden = "continuation"]
    Table,
    /// The position.
    Position,
}

/// Advance the target's sequence behind the identity column `column` of
/// `target` past everything the clone copied into it and everything the same
/// column's sequence of `source` minted.
///
/// # Errors
///
/// Returns [`ContextStoreError`] when the sequence position cannot be read or
/// set.
pub(crate) async fn continue_identity(conn: &mut PgConnection, source: &TableName, target: &TableName, column: &DynIden) -> Result<(), ContextStoreError> {
    continuation_statement(source, target, column).to_query().execute(conn).await?;
    Ok(())
}

/// `SELECT setval(<target sequence>, continuation.position) FROM (SELECT
/// GREATEST(pg_sequence_last_value(<source sequence>), (SELECT max(column) FROM
/// target)) AS position) AS continuation WHERE continuation.position IS NOT
/// NULL`.
///
/// The source sequence's last value may run ahead of the last value handed out
/// when the sequence caches, which only moves the continuation further on; it
/// is NULL for a sequence never called, which `GREATEST` ignores.
fn continuation_statement(source: &TableName, target: &TableName, column: &DynIden) -> SelectStatement {
    let copied_maximum = scalar_subquery(Query::select().expr(Func::max(Expr::col(column.clone()))).from(target.clone()).to_owned());
    let position = Func::greatest([sequence_last_value(identity_sequence(source, column)), copied_maximum]);
    Query::select()
        .expr(set_sequence_position(
            identity_sequence(target, column),
            Expr::col((Continuation::Table, Continuation::Position)),
        ))
        .from_subquery(Query::select().expr_as(position, Continuation::Position).to_owned(), Continuation::Table)
        .and_where(Expr::col((Continuation::Table, Continuation::Position)).is_not_null())
        .to_owned()
}

#[cfg(test)]
mod tests {
    use crate::snapshot::clone::identity_continuation::continuation_statement;
    use sea_query::{Alias, IntoIden, PostgresQueryBuilder, TableName};

    #[test]
    fn the_target_sequence_is_set_from_the_larger_of_the_source_position_and_the_copied_maximum() {
        let sql = continuation_statement(
            &TableName::from((Alias::new("snap_2"), Alias::new("entities"))),
            &TableName::from((Alias::new("snap_3_stg"), Alias::new("entities"))),
            &Alias::new("entity_key").into_iden(),
        )
        .to_string(PostgresQueryBuilder);
        pretty_assertions::assert_eq!(
            sql,
            r#"SELECT setval(pg_get_serial_sequence('"snap_3_stg"."entities"', 'entity_key')::regclass, "continuation"."position") FROM (SELECT GREATEST(pg_sequence_last_value(pg_get_serial_sequence('"snap_2"."entities"', 'entity_key')::regclass), (SELECT MAX("entity_key") FROM "snap_3_stg"."entities")) AS "position") AS "continuation" WHERE "continuation"."position" IS NOT NULL"#
        );
    }

    /// The source position names the source column's sequence alone and no
    /// catalogue of every sequence, so another schema's sequences are never
    /// read.
    #[test]
    fn the_source_position_reads_no_sequence_catalogue() {
        let sql = continuation_statement(
            &TableName::from((Alias::new("snap_2"), Alias::new("entities"))),
            &TableName::from((Alias::new("snap_3_stg"), Alias::new("entities"))),
            &Alias::new("entity_key").into_iden(),
        )
        .to_string(PostgresQueryBuilder);
        assert!(!sql.contains("pg_sequences"), "{sql}");
    }
}
