//! The sequence behind an identity column, read and moved by functions
//! sea-query has no constructor for.

use crate::{cast::cast, identifier::quoted_table, pg_type::PostgresType};
use sea_query::{DynIden, Expr, Func, Iden, SimpleExpr, TableName};

/// The sequence functions.
#[derive(Iden, Clone, Copy)]
enum SequenceFunction {
    /// `pg_get_serial_sequence`.
    PgGetSerialSequence,
    /// `pg_sequence_last_value`.
    PgSequenceLastValue,
    /// `setval`.
    Setval,
}

/// `pg_get_serial_sequence('<table>', '<column>')::regclass`: the sequence
/// behind the identity column `column` of `table`.
///
/// The relation is passed as its quoted name, which the function parses by
/// the usual identifier rules; the column is taken literally.
#[must_use]
pub fn identity_sequence(table: &TableName, column: &DynIden) -> SimpleExpr {
    let sequence = Func::cust(SequenceFunction::PgGetSerialSequence)
        .args([
            SimpleExpr::from(Expr::val(quoted_table(table))),
            SimpleExpr::from(Expr::val(column.to_string())),
        ])
        .into();
    cast(sequence, PostgresType::Regclass)
}

/// `pg_sequence_last_value(sequence)`: the last value `sequence` handed out,
/// NULL when it was never called.
///
/// A sequence that caches may report a value ahead of the last one a session
/// used.
#[must_use]
pub fn sequence_last_value(sequence: SimpleExpr) -> SimpleExpr {
    Func::cust(SequenceFunction::PgSequenceLastValue).arg(sequence).into()
}

/// `setval(sequence, position)`: the next value `sequence` hands out follows
/// `position`.
#[must_use]
pub fn set_sequence_position(sequence: SimpleExpr, position: SimpleExpr) -> SimpleExpr {
    Func::cust(SequenceFunction::Setval).args([sequence, position]).into()
}

#[cfg(test)]
mod tests {
    use crate::sequence::{identity_sequence, sequence_last_value, set_sequence_position};
    use sea_query::{Alias, IntoIden, PostgresQueryBuilder, Query, TableName};

    #[test]
    fn an_identity_sequence_is_found_by_the_quoted_relation_and_the_literal_column() {
        let sequence = identity_sequence(
            &TableName::from((Alias::new("snap_2"), Alias::new("entities"))),
            &Alias::new("entity_key").into_iden(),
        );
        let sql = Query::select()
            .expr(set_sequence_position(sequence.clone(), sequence_last_value(sequence)))
            .to_string(PostgresQueryBuilder);
        pretty_assertions::assert_eq!(
            sql,
            r#"SELECT setval(pg_get_serial_sequence('"snap_2"."entities"', 'entity_key')::regclass, pg_sequence_last_value(pg_get_serial_sequence('"snap_2"."entities"', 'entity_key')::regclass))"#
        );
    }
}
