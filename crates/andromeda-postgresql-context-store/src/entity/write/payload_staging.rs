//! The stored document a set-write lands, staged apart from the projection.
//!
//! Every relation statement of a set-write reads the projection `proj`, so
//! PostgreSQL materialises it once per statement. The stored document is read
//! by the document write alone, and is by far the widest value of a row: staged
//! inside the projection, every payload would be copied into that
//! materialisation (spilled to disk past `work_mem`) and read back once by
//! every sibling that only wanted a key or a geometry. The payload is staged in
//! a source of its own instead, `pay`, which only the document write reads.
//!
//! `pay` carries the entity identifier beside the payload, and for an update
//! the expectation its compare-and-set holds, so the document write finds its
//! row by key: the update looks the identifier up through the unique index on
//! `entities.id` and never reads `proj`, the insert pairs `pay` with the
//! identity insert's returned rows the way its sibling inserts pair `proj`.
//! Nothing pairs rows by their position in the batch: a positional pairing has
//! no index, and under a generic plan's row estimate PostgreSQL rescans the
//! payload source once per entity, which is quadratic in the batch size.

use crate::entity::write::staging::StagedColumn;
use andromeda_postgresql_schema::current::Entities;
use andromeda_postgresql_seaquery::{
    cast::cast,
    pg_type::PostgresType,
    placeholder::external_placeholder,
    unnest::{UnnestColumn, apply_column_alias, unnest_call},
};
use sea_query::{CommonTableExpression, Expr, ExprTrait, Iden, IntoIden, Query, SelectStatement, SimpleExpr};

/// The payload staging alias, `p`.
///
/// Spelled nowhere else in a statement, so the single `AS "p"` its table source
/// emits is the one [`apply_payload_alias`] rewrites.
#[derive(Iden, Clone, Copy)]
#[iden = "p"]
struct PayloadStaged;

/// The payload CTE, `pay`: one stored document per staged entity, keyed by the
/// entity's identifier.
#[derive(Iden, Clone, Copy)]
#[iden = "pay"]
pub(super) struct Pay;

/// The bind every set-write stages its identifiers at. The projection and the
/// payload source both read it, so the identifiers cross the wire once.
const IDENTIFIER_POSITION: usize = 1;

/// Which document write a payload source serves, with the binds it reads
/// besides the identifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DocumentWrite {
    /// The insert's document write: identifier and payload.
    Insert {
        /// The bind carrying the payloads.
        payload: usize,
    },
    /// The update's document write: identifier, the expectation its
    /// compare-and-set holds, and payload.
    Update {
        /// The bind carrying the expectations.
        expectation: usize,
        /// The bind carrying the payloads.
        payload: usize,
    },
}

/// One staging column of the payload source and the bind it reads.
#[derive(Clone, Copy)]
struct PayloadColumn {
    /// The column, named as the `AS p(…)` alias spells it.
    column: StagedColumn,
    /// The array type its bind is cast to.
    array_type: PostgresType,
    /// The bind carrying it.
    position: usize,
}

impl PayloadColumn {
    /// The column as the `UNNEST` source stages it.
    fn unnested(self) -> UnnestColumn {
        UnnestColumn::new(self.column, self.array_type)
    }
}

/// The staging columns of the payload source, in their `AS p(…)` order.
fn payload_columns(write: DocumentWrite) -> Vec<PayloadColumn> {
    let identifier = PayloadColumn {
        column: StagedColumn::Id,
        array_type: PostgresType::TextArray,
        position: IDENTIFIER_POSITION,
    };
    let payload = |position| PayloadColumn {
        column: StagedColumn::Payload,
        array_type: PostgresType::TextArray,
        position,
    };
    match write {
        DocumentWrite::Insert { payload: position } => vec![identifier, payload(position)],
        DocumentWrite::Update {
            expectation,
            payload: position,
        } => vec![
            identifier,
            PayloadColumn {
                column: StagedColumn::ExpectedModifiedAt,
                array_type: PostgresType::TimestamptzArray,
                position: expectation,
            },
            payload(position),
        ],
    }
}

/// The payload CTE for `write`, for the update:
/// `pay AS (SELECT p.id, p.expected_modified_at, p.payload FROM UNNEST($1::text[], $e::timestamptz[], $n::text[]) AS p(id, expected_modified_at, payload))`.
///
/// It is read by the document write alone, so PostgreSQL inlines it there
/// rather than materialising it.
pub(super) fn payload_cte(write: DocumentWrite) -> CommonTableExpression {
    let columns = payload_columns(write);
    let unnested: Vec<UnnestColumn> = columns.iter().map(|column| column.unnested()).collect();
    let mut select = Query::select();
    for column in &columns {
        select.expr(Expr::col((PayloadStaged, column.column)));
    }
    let select: SelectStatement = select
        .from_function(
            unnest_call(
                unnested
                    .iter()
                    .zip(&columns)
                    .map(|(unnested, column)| (unnested, external_placeholder(column.position))),
            ),
            PayloadStaged,
        )
        .take();
    CommonTableExpression::new().table_name(Pay).query(select).to_owned()
}

/// The document a write stores: `pay.payload::jsonb`.
pub(super) fn staged_payload() -> SimpleExpr {
    cast(Expr::col((Pay, StagedColumn::Payload)), PostgresType::Jsonb)
}

/// The expectation the update's compare-and-set holds:
/// `pay.expected_modified_at`.
pub(super) fn staged_expectation() -> SimpleExpr {
    Expr::col((Pay, StagedColumn::ExpectedModifiedAt))
}

/// The payload of the entity `relation` names: `pay.id = <relation>.id`.
pub(super) fn payload_of(relation: impl IntoIden) -> SimpleExpr {
    Expr::col((Pay, Entities::Id)).equals((relation.into_iden(), Entities::Id))
}

/// Rewrite the payload source's `AS "p"` into `AS p(<columns>)`.
pub(super) fn apply_payload_alias(sql: String, write: DocumentWrite) -> String {
    let columns: Vec<UnnestColumn> = payload_columns(write).into_iter().map(PayloadColumn::unnested).collect();
    apply_column_alias(sql, &PayloadStaged, &columns)
}

#[cfg(test)]
mod tests {
    use crate::entity::write::{
        payload_staging::{DocumentWrite, apply_payload_alias, payload_cte, payload_of, staged_expectation, staged_payload},
        staging::Ins,
    };
    use sea_query::{PostgresQueryBuilder, Query, WithClause};

    /// Render `write`'s payload CTE in front of a select reading it.
    fn rendered(write: DocumentWrite) -> String {
        let mut with = WithClause::new();
        with.cte(payload_cte(write));
        apply_payload_alias(
            with.query(Query::select().expr(staged_payload()).and_where(payload_of(Ins)).take())
                .to_string(PostgresQueryBuilder),
            write,
        )
    }

    #[test]
    fn the_insert_payload_is_staged_with_its_identifier_and_cast_where_it_is_stored() {
        let sql = rendered(DocumentWrite::Insert { payload: 13 });
        assert!(
            sql.starts_with(r#"WITH "pay" AS (SELECT "p"."id", "p"."payload" FROM UNNEST($1::text[], $13::text[]) AS p(id, payload))"#),
            "{sql}"
        );
        assert!(sql.contains(r#"SELECT "pay"."payload"::jsonb WHERE "pay"."id" = "ins"."id""#), "{sql}");
    }

    #[test]
    fn the_update_payload_carries_the_expectation_its_compare_and_set_holds() {
        let sql = rendered(DocumentWrite::Update { expectation: 16, payload: 17 });
        assert!(
            sql.starts_with(r#"WITH "pay" AS (SELECT "p"."id", "p"."expected_modified_at", "p"."payload" FROM UNNEST($1::text[], $16::timestamptz[], $17::text[]) AS p(id, expected_modified_at, payload))"#),
            "{sql}"
        );
        let expectation = Query::select().expr(staged_expectation()).to_string(PostgresQueryBuilder);
        pretty_assertions::assert_eq!(expectation, r#"SELECT "pay"."expected_modified_at""#);
    }

    /// No source is paired by position: a positional pairing has no index, so a
    /// generic plan rescans the payload source once per entity.
    #[test]
    fn the_payload_is_paired_by_identifier_never_by_position() {
        for write in [DocumentWrite::Insert { payload: 13 }, DocumentWrite::Update { expectation: 16, payload: 17 }] {
            let sql = rendered(write);
            assert!(!sql.contains("ORDINALITY"), "{sql}");
            assert!(!sql.contains("ordinality"), "{sql}");
        }
    }
}
