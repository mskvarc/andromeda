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

use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_postgresql_query::history_source::HistoryOf;
use andromeda_postgresql_seaquery::unnest::unnest_of;
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
    let ids: Vec<String> = entity_ids.iter().map(Identifier::to_string).collect();
    // clone: the identifier is both projected and the Entity every source of the read narrows to.
    let mut read = read(&HistoryOf::Entity(entity.clone()));
    read.offset(0);
    let mut select = Query::select();
    select
        .expr_as(entity, Alias::new(READ_ENTITY_ID))
        .from_function(unnest_of(Expr::val(ids)), ReadEntity)
        .join_lateral(JoinType::Join, read, alias, SimpleExpr::from(true))
        .order_by((ReadEntity, ReadEntity), Order::Asc);
    select
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    clippy::panic,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::entity_reads::each_entity;
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_postgresql_query::history_source::HistoryOf;
    use sea_query::{Alias, Asterisk, Expr, ExprTrait, PostgresQueryBuilder, Query, Value};

    fn ids() -> Vec<Identifier> {
        ["urn:ngsi-ld:Vehicle:1", "urn:ngsi-ld:Vehicle:2"]
            .iter()
            .map(|id| id.parse().expect("a valid identifier"))
            .collect()
    }

    /// Every identifier binds in one array, so the statement's parameters do
    /// not grow with the page, and the read narrows to the unnested column.
    #[test]
    fn every_entity_is_bound_in_one_array_and_read_in_a_lateral() {
        let statement = each_entity(&ids(), Alias::new("h"), |of| {
            let HistoryOf::Entity(entity) = of else {
                panic!("the read names one Entity: {of:?}");
            };
            Query::select()
                .column(Asterisk)
                .from(Alias::new("t"))
                .and_where(Expr::col(Alias::new("id")).eq(entity.clone()))
                .take()
        });
        let (sql, values) = statement.build(PostgresQueryBuilder);
        pretty_assertions::assert_eq!(
            sql,
            r#"SELECT "read_entity"."read_entity" AS "read_entity_id" FROM UNNEST($1) AS "read_entity" JOIN LATERAL (SELECT * FROM "t" WHERE "id" = "read_entity"."read_entity" OFFSET $2) AS "h" ON $3 ORDER BY "read_entity"."read_entity" ASC"#
        );
        assert!(matches!(values.0.first(), Some(Value::Array(_, Some(ids))) if ids.len() == 2), "{values:?}");
    }
}
