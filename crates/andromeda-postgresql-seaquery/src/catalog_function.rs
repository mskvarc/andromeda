//! System information and administration functions sea-query has no
//! constructor for: the definitions the catalog renders back as text, the size
//! of a stored value, the backend a session runs as, and the set-returning
//! series a fixture generates rows from.

use sea_query::{Func, FunctionCall, Iden, SimpleExpr};

/// The catalog and administration functions.
#[derive(Iden, Clone, Copy)]
enum CatalogFunction {
    /// `bool_and`.
    BoolAnd,
    /// `chr`.
    Chr,
    /// `generate_series`.
    GenerateSeries,
    /// `host`.
    Host,
    /// `inet_client_addr`.
    InetClientAddr,
    /// `left`.
    Left,
    /// `lpad`.
    Lpad,
    /// `md5`.
    Md5,
    /// `num_nonnulls`.
    NumNonnulls,
    /// `octet_length`.
    OctetLength,
    /// `pg_cancel_backend`.
    PgCancelBackend,
    /// `pg_column_size`.
    PgColumnSize,
    /// `gen_random_uuid`.
    GenRandomUuid,
    /// `pg_get_constraintdef`.
    PgGetConstraintdef,
    /// `pg_get_expr`.
    PgGetExpr,
    /// `pg_get_indexdef`.
    PgGetIndexdef,
    /// `pg_sleep`.
    PgSleep,
    /// `pg_stat_get_xact_tuples_hot_updated`.
    PgStatGetXactTuplesHotUpdated,
    /// `pg_stat_get_xact_tuples_updated`.
    PgStatGetXactTuplesUpdated,
    /// `pg_terminate_backend`.
    PgTerminateBackend,
}

/// Call `function` with `arguments`.
fn call(function: CatalogFunction, arguments: impl IntoIterator<Item = SimpleExpr>) -> SimpleExpr {
    Func::cust(function).args(arguments).into()
}

/// `bool_and(predicate)`: true when the predicate holds on every row of the
/// group, NULL for an empty group.
#[must_use]
pub fn bool_and(predicate: SimpleExpr) -> SimpleExpr {
    call(CatalogFunction::BoolAnd, [predicate])
}

/// `chr(code_point)`: the character with the given Unicode code point.
#[must_use]
pub fn chr(code_point: SimpleExpr) -> SimpleExpr {
    call(CatalogFunction::Chr, [code_point])
}

/// `generate_series(first, last)`: one row per integer from `first` to
/// `last`, for a table source.
#[must_use]
pub fn generate_series(first: SimpleExpr, last: SimpleExpr) -> FunctionCall {
    Func::cust(CatalogFunction::GenerateSeries).args([first, last])
}

/// `gen_random_uuid()`: a fresh random version 4 UUID.
#[must_use]
pub fn random_uuid() -> SimpleExpr {
    call(CatalogFunction::GenRandomUuid, [])
}

/// `host(address)`: the address of an `inet` value as text, without a netmask.
#[must_use]
pub fn host(address: SimpleExpr) -> SimpleExpr {
    call(CatalogFunction::Host, [address])
}

/// `host(inet_client_addr())`: the address the session's client connected
/// from, without a netmask.
#[must_use]
pub fn client_host() -> SimpleExpr {
    host(call(CatalogFunction::InetClientAddr, []))
}

/// `left(text, count)`: the first `count` characters of `text`.
#[must_use]
pub fn left(text: SimpleExpr, count: SimpleExpr) -> SimpleExpr {
    call(CatalogFunction::Left, [text, count])
}

/// `lpad(text, length, fill)`: `text` padded on the left with `fill` to
/// `length` characters, or cut to it.
#[must_use]
pub fn lpad(text: SimpleExpr, length: SimpleExpr, fill: SimpleExpr) -> SimpleExpr {
    call(CatalogFunction::Lpad, [text, length, fill])
}

/// `md5(text)`: the MD5 digest of `text`, as 32 hexadecimal characters.
#[must_use]
pub fn md5(text: SimpleExpr) -> SimpleExpr {
    call(CatalogFunction::Md5, [text])
}

/// `num_nonnulls(value, ...)`: how many of the arguments are not NULL.
#[must_use]
pub fn num_nonnulls(values: impl IntoIterator<Item = SimpleExpr>) -> SimpleExpr {
    call(CatalogFunction::NumNonnulls, values)
}

/// `octet_length(value)`: the number of bytes `value` occupies.
#[must_use]
pub fn octet_length(value: SimpleExpr) -> SimpleExpr {
    call(CatalogFunction::OctetLength, [value])
}

/// `pg_cancel_backend(pid)`: cancel the statement the backend `pid` runs.
#[must_use]
pub fn cancel_backend(pid: SimpleExpr) -> SimpleExpr {
    call(CatalogFunction::PgCancelBackend, [pid])
}

/// `pg_terminate_backend(pid)`: close the session of the backend `pid`.
#[must_use]
pub fn terminate_backend(pid: SimpleExpr) -> SimpleExpr {
    call(CatalogFunction::PgTerminateBackend, [pid])
}

/// `pg_column_size(value)`: the bytes a stored value, or a whole row when
/// given a row reference, occupies.
#[must_use]
pub fn column_size(value: SimpleExpr) -> SimpleExpr {
    call(CatalogFunction::PgColumnSize, [value])
}

/// `pg_get_constraintdef(oid)`: the definition of the constraint `oid`, as the
/// text `ALTER TABLE ... ADD CONSTRAINT` takes.
#[must_use]
pub fn constraint_definition(oid: SimpleExpr) -> SimpleExpr {
    call(CatalogFunction::PgGetConstraintdef, [oid])
}

/// `pg_get_indexdef(oid)`: the `CREATE INDEX` statement that rebuilds the
/// index `oid`.
#[must_use]
pub fn index_definition(oid: SimpleExpr) -> SimpleExpr {
    call(CatalogFunction::PgGetIndexdef, [oid])
}

/// `pg_get_expr(tree, relation)`: the SQL text of a stored expression tree,
/// such as an index predicate, read in the context of the relation `oid`.
#[must_use]
pub fn expression_definition(tree: SimpleExpr, relation: SimpleExpr) -> SimpleExpr {
    call(CatalogFunction::PgGetExpr, [tree, relation])
}

/// `pg_sleep(seconds)`: hold the session for `seconds`.
#[must_use]
pub fn pg_sleep(seconds: SimpleExpr) -> SimpleExpr {
    call(CatalogFunction::PgSleep, [seconds])
}

/// `pg_stat_get_xact_tuples_updated(relation)`: the rows of the relation `oid`
/// the current transaction has updated.
#[must_use]
pub fn transaction_tuples_updated(relation: SimpleExpr) -> SimpleExpr {
    call(CatalogFunction::PgStatGetXactTuplesUpdated, [relation])
}

/// `pg_stat_get_xact_tuples_hot_updated(relation)`: the rows of the relation
/// the current transaction has updated without touching an index.
#[must_use]
pub fn transaction_tuples_hot_updated(relation: SimpleExpr) -> SimpleExpr {
    call(CatalogFunction::PgStatGetXactTuplesHotUpdated, [relation])
}

#[cfg(test)]
mod tests {
    use crate::catalog_function::{
        bool_and,
        cancel_backend,
        chr,
        client_host,
        column_size,
        constraint_definition,
        expression_definition,
        generate_series,
        index_definition,
        left,
        lpad,
        md5,
        num_nonnulls,
        octet_length,
        pg_sleep,
        random_uuid,
        terminate_backend,
        transaction_tuples_hot_updated,
        transaction_tuples_updated,
    };
    use pretty_assertions::assert_eq;
    use sea_query::{Alias, Expr, PostgresQueryBuilder, Query};

    fn rendered(expr: sea_query::SimpleExpr) -> String {
        Query::select().expr(expr).to_string(PostgresQueryBuilder)
    }

    #[test]
    fn the_definition_functions_take_the_object_identifier() {
        assert_eq!(
            rendered(constraint_definition(Expr::col(Alias::new("oid")))),
            r#"SELECT pg_get_constraintdef("oid")"#
        );
        assert_eq!(rendered(index_definition(Expr::col(Alias::new("oid")))), r#"SELECT pg_get_indexdef("oid")"#);
        assert_eq!(
            rendered(expression_definition(Expr::col(Alias::new("indpred")), Expr::col(Alias::new("indrelid")))),
            r#"SELECT pg_get_expr("indpred", "indrelid")"#
        );
    }

    #[test]
    fn the_size_and_nullness_functions_take_their_operands_positionally() {
        assert_eq!(rendered(column_size(Expr::col(Alias::new("t")))), r#"SELECT pg_column_size("t")"#);
        assert_eq!(rendered(octet_length(Expr::col(Alias::new("id")))), r#"SELECT octet_length("id")"#);
        assert_eq!(rendered(left(Expr::col(Alias::new("id")), Expr::val(1))), r#"SELECT left("id", 1)"#);
        assert_eq!(
            rendered(num_nonnulls([Expr::col(Alias::new("a")), Expr::col(Alias::new("b"))])),
            r#"SELECT num_nonnulls("a", "b")"#
        );
    }

    #[test]
    fn the_backend_functions_render_with_their_identifier_argument() {
        assert_eq!(rendered(cancel_backend(Expr::col(Alias::new("pid")))), r#"SELECT pg_cancel_backend("pid")"#);
        assert_eq!(
            rendered(terminate_backend(Expr::col(Alias::new("pid")))),
            r#"SELECT pg_terminate_backend("pid")"#
        );
        assert_eq!(rendered(pg_sleep(Expr::val(20))), "SELECT pg_sleep(20)");
        assert_eq!(rendered(client_host()), "SELECT host(inet_client_addr())");
        assert_eq!(rendered(chr(Expr::val(65))), "SELECT chr(65)");
        assert_eq!(
            rendered(lpad(Expr::col(Alias::new("n")), Expr::val(6), Expr::val("0"))),
            r#"SELECT lpad("n", 6, '0')"#
        );
        assert_eq!(rendered(random_uuid()), "SELECT gen_random_uuid()");
        assert_eq!(rendered(md5(Expr::col(Alias::new("n")))), r#"SELECT md5("n")"#);
    }

    #[test]
    fn the_aggregate_and_series_functions_render_their_calls() {
        assert_eq!(rendered(bool_and(Expr::col(Alias::new("ok")))), r#"SELECT bool_and("ok")"#);
        assert_eq!(
            Query::select()
                .column(Alias::new("n"))
                .from_function(generate_series(Expr::val(1), Expr::val(5)), Alias::new("n"))
                .to_string(PostgresQueryBuilder),
            r#"SELECT "n" FROM generate_series(1, 5) AS "n""#
        );
    }

    #[test]
    fn the_transaction_tuple_counters_take_the_relation_identifier() {
        assert_eq!(
            rendered(transaction_tuples_updated(Expr::col(Alias::new("oid")))),
            r#"SELECT pg_stat_get_xact_tuples_updated("oid")"#
        );
        assert_eq!(
            rendered(transaction_tuples_hot_updated(Expr::col(Alias::new("oid")))),
            r#"SELECT pg_stat_get_xact_tuples_hot_updated("oid")"#
        );
    }
}
