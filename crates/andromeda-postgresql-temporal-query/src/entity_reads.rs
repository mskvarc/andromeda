//! Statements that read the history of several Entities at once, each row
//! naming the Entity it was read for.
//!
//! A temporal query reads every Entity of its page in one statement rather
//! than one statement per Entity: the identifiers are bound as one array and
//! each Entity's history is read in a LATERAL over it, so every per-Entity
//! read keeps the stream-led index access a read of one Entity has, and the
//! number of statements does not grow with the page. The rows come ordered by
//! the Entity first, so the store cuts them into one run per Entity and
//! decodes each run on its own.
//!
//! The array is costed by a custom plan from the identifiers it holds and by
//! a generic plan as ten, so under `plan_cache_mode = auto` the generic plan
//! of a bound array never looks as cheap as the custom plans and the
//! statement is planned on every execution. A page of one identifier, which
//! every single-Entity retrieve reads, unnests `ARRAY[$n]` instead: both plans
//! count its one element, and the plan cache keeps the generic plan with no
//! setting of its own. A page of several stays one bound array, so the number
//! of statement texts does not grow with the page size, and is planned
//! generically on purpose.

use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_postgresql_query::history_source::HistoryOf;
use andromeda_postgresql_seaquery::{pg_array::array_literal, statement_planning::StatementPlanning, unnest::unnest_of};
use sea_query::{Alias, Expr, Iden, IntoIden, JoinType, Order, Query, SelectStatement, SimpleExpr};

/// The name every statement reading several Entities projects each row's
/// Entity identifier under.
pub const READ_ENTITY_ID: &str = "read_entity_id";

/// Alias `read_entity`: one identifier of the unnested array, which is also
/// the name of its only column.
#[derive(Iden, Clone, Copy)]
#[iden = "read_entity"]
struct ReadEntity;

/// The statement reading `read` for each of `entity_ids`, its rows under
/// `alias`, beside the identifier of the Entity they were read for, ordered
/// by it.
///
/// `read` is given the Entity of the row it is read for, a column of the
/// unnested array the LATERAL reads, so it narrows every source it reads
/// inside. The caller projects what it reads under `alias` and orders the
/// rows of one Entity after the identifier.
///
/// The read ends in `OFFSET 0`, so PostgreSQL keeps it a subquery run once
/// per identifier rather than pulling it up into the outer join. Pulled up,
/// the identifier becomes a join clause the planner may apply last: it then
/// reads every stream of the branch, and runs each stream's page read, before
/// it drops the streams of the Entities the page does not name.
pub(crate) fn each_entity(entity_ids: &[Identifier], alias: impl IntoIden, read: impl FnOnce(&HistoryOf) -> SelectStatement) -> SelectStatement {
    let entity = Expr::col((ReadEntity, ReadEntity));
    let identifiers = match entity_ids {
        [entity_id] => array_literal([Expr::val(entity_id.to_string())]),
        [] | [_, _, ..] => Expr::val(entity_ids.iter().map(Identifier::to_string).collect::<Vec<String>>()),
    };
    // clone: the identifier is both projected and the Entity every source of the read narrows to.
    let mut read = read(&HistoryOf::Entity(entity.clone()));
    read.offset(0);
    let mut select = Query::select();
    select
        .expr_as(entity, Alias::new(READ_ENTITY_ID))
        .from_function(unnest_of(identifiers), ReadEntity)
        .join_lateral(JoinType::Join, read, alias, SimpleExpr::from(true))
        .order_by((ReadEntity, ReadEntity), Order::Asc);
    select
}

/// How PostgreSQL may plan a statement [`each_entity`] builds over
/// `entity_ids`: the plan cache's own choice for one identifier, whose
/// generic plan it adopts, and the generic plan for any other number.
#[must_use]
pub const fn entity_read_planning(entity_ids: &[Identifier]) -> StatementPlanning {
    match entity_ids {
        [_] => StatementPlanning::Cached,
        [] | [_, _, ..] => StatementPlanning::Generic,
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    clippy::panic,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::entity_reads::{each_entity, entity_read_planning};
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_postgresql_query::history_source::HistoryOf;
    use andromeda_postgresql_seaquery::statement_planning::StatementPlanning;
    use sea_query::{Alias, Asterisk, Expr, ExprTrait, PostgresQueryBuilder, Query, SelectStatement, Value};

    fn ids() -> Vec<Identifier> {
        ["urn:ngsi-ld:Vehicle:1", "urn:ngsi-ld:Vehicle:2"]
            .iter()
            .map(|id| id.parse().expect("a valid identifier"))
            .collect()
    }

    fn read_of(entity_ids: &[Identifier]) -> SelectStatement {
        each_entity(entity_ids, Alias::new("h"), |of| {
            let HistoryOf::Entity(entity) = of else {
                panic!("the read names one Entity: {of:?}");
            };
            Query::select()
                .column(Asterisk)
                .from(Alias::new("t"))
                .and_where(Expr::col(Alias::new("id")).eq(entity.clone()))
                .take()
        })
    }

    /// Every identifier binds in one array, so the statement's parameters do
    /// not grow with the page, and the read narrows to the unnested column.
    #[test]
    fn every_entity_is_bound_in_one_array_and_read_in_a_lateral() {
        let (sql, values) = read_of(&ids()).build(PostgresQueryBuilder);
        pretty_assertions::assert_eq!(
            sql,
            r#"SELECT "read_entity"."read_entity" AS "read_entity_id" FROM UNNEST($1) AS "read_entity" JOIN LATERAL (SELECT * FROM "t" WHERE "id" = "read_entity"."read_entity" OFFSET $2) AS "h" ON $3 ORDER BY "read_entity"."read_entity" ASC"#
        );
        assert!(matches!(values.0.first(), Some(Value::Array(_, Some(ids))) if ids.len() == 2), "{values:?}");
        assert_eq!(entity_read_planning(&ids()), StatementPlanning::Generic);
    }

    /// One identifier binds as the one element of an array constructor, which
    /// a generic plan counts as the one row it is.
    #[test]
    fn one_entity_binds_its_identifier_as_the_element_of_an_array_constructor() {
        let one = &ids()[..1];
        let (sql, values) = read_of(one).build(PostgresQueryBuilder);
        pretty_assertions::assert_eq!(
            sql,
            r#"SELECT "read_entity"."read_entity" AS "read_entity_id" FROM UNNEST(ARRAY[ $1 ]) AS "read_entity" JOIN LATERAL (SELECT * FROM "t" WHERE "id" = "read_entity"."read_entity" OFFSET $2) AS "h" ON $3 ORDER BY "read_entity"."read_entity" ASC"#
        );
        pretty_assertions::assert_eq!(values.0.first(), Some(&Value::from("urn:ngsi-ld:Vehicle:1")));
        pretty_assertions::assert_eq!(entity_read_planning(one), StatementPlanning::Cached);
    }

    #[test]
    fn an_empty_page_reads_an_empty_array_planned_generically() {
        let (sql, values) = read_of(&[]).build(PostgresQueryBuilder);
        assert!(sql.contains(r#"FROM UNNEST($1) AS "read_entity""#), "{sql}");
        assert!(matches!(values.0.first(), Some(Value::Array(_, Some(ids))) if ids.is_empty()), "{values:?}");
        pretty_assertions::assert_eq!(entity_read_planning(&[]), StatementPlanning::Generic);
    }
}
