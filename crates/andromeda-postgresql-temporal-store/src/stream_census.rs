//! Counting streams into the stream type census.
//!
//! A temporal selection by attribute alone pages the holders of the attribute
//! by walking the Evolution memberships of every type holding it, and
//! `stream_type_census` is what names those types: per type and attribute,
//! how many streams of the attribute the Evolutions recorded under the type
//! hold. A stream counts under every type its Evolution is recorded under;
//! those only grow.
//!
//! Every statement that inserts or deletes streams adds its change from its
//! own `RETURNING`, under the types it reads from the identity rows it locks
//! `FOR SHARE`, and a type an existing Evolution gains is counted after the
//! statement giving it, while the evolution upsert holds the row, over the
//! streams the Evolution then holds ([`gained_types_census`]). The identity
//! row lock orders the two: a stream change waiting on the upsert reads the
//! gained type, and an upsert waiting on a stream change counts the streams
//! that change committed, so a stream and a type written concurrently are
//! counted together exactly once.

use andromeda_postgresql_schema::{
    current::Entities,
    temporal::{StreamTypeCensus, TemporalAttributeStreams},
};
use andromeda_postgresql_seaquery::{
    aggregate::count_all,
    branch::qualified,
    literal::true_literal,
    pg_type::PostgresType,
    unnest::{UnnestAliased, UnnestColumn, unnest_call, unnest_of},
};
use sea_query::{
    CommonTableExpression,
    DeleteStatement,
    DynIden,
    Expr,
    ExprTrait,
    Iden,
    InsertStatement,
    JoinType,
    LockType,
    Order,
    Query,
    SelectStatement,
    SimpleExpr,
    WithClause,
    WithQuery,
    error::Error as SeaQueryError,
};

/// Alias `counted`: one stream under one type, as a census change reads it.
#[derive(Iden, Clone, Copy)]
#[iden = "counted"]
struct Counted;

/// Alias `typed`: one type of a stream's Evolution, unnested.
#[derive(Iden, Clone, Copy)]
#[iden = "typed"]
struct Typed;

/// The streams a statement inserted or deleted, `changed_streams`.
#[derive(Iden, Clone, Copy)]
#[iden = "changed_streams"]
struct ChangedStreams;

/// The identity rows of the changed streams' Entities, locked, `owners`.
#[derive(Iden, Clone, Copy)]
#[iden = "owners"]
struct Owners;

/// The census change of the streams a statement deleted,
/// `census_change`, whose only purpose is to run.
#[derive(Iden, Clone, Copy)]
#[iden = "census_change"]
struct CensusChange;

/// Alias `gained`: one type an existing Evolution gained, unnested.
#[derive(Iden, Clone, Copy)]
#[iden = "gained"]
struct Gained;

/// Alias `gs`: a stream of an Evolution that gained a type.
#[derive(Iden, Clone, Copy)]
#[iden = "gs"]
struct GainingStream;

/// Which way a census change moves the count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StreamChange {
    /// The streams were inserted, or gained a type.
    Added,
    /// The streams were deleted.
    Removed,
}

impl StreamChange {
    /// `COUNT(*)` or its negation.
    fn delta(self) -> SimpleExpr {
        match self {
            StreamChange::Added => count_all(),
            StreamChange::Removed => count_all().mul(-1),
        }
    }
}

/// One type an Evolution that existed before a statement gained in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct GainedType {
    /// The surrogate key of the Entity holding the Evolution.
    pub(crate) entity_key: i64,
    /// The dictionary key of the type it gained.
    pub(crate) type_key: i32,
}

/// The census rows of `rows`, one per stream and type it counts under, read
/// as `(entity_type_name_key, attribute_name_key)`, inserted into the census
/// of the branch `schema` names as one signed row per type and attribute.
///
/// ```sql
/// INSERT INTO stream_type_census (entity_type_name_key, attribute_name_key, stream_delta)
/// SELECT counted.entity_type_name_key, counted.attribute_name_key, ±COUNT(*)
/// FROM (rows) AS counted GROUP BY counted.entity_type_name_key, counted.attribute_name_key
/// ```
///
/// # Errors
///
/// Returns [`SeaQueryError`] if the projection ever stops matching the column
/// list, which the fixed shape here makes unreachable.
pub(crate) fn stream_census_change(schema: Option<&DynIden>, rows: SelectStatement, change: StreamChange) -> Result<InsertStatement, SeaQueryError> {
    let keys = [StreamTypeCensus::EntityTypeNameKey, StreamTypeCensus::AttributeNameKey];
    let mut counted = Query::select();
    for key in keys {
        counted.column((Counted, key)).group_by_col((Counted, key));
    }
    counted.expr(change.delta()).from_subquery(rows, Counted);
    let mut insert = Query::insert();
    insert
        .into_table(qualified(schema, StreamTypeCensus::Table))
        .columns(keys.into_iter().chain([StreamTypeCensus::StreamDelta]))
        .select_from(counted.take())?;
    Ok(insert)
}

/// `addition`, an insert of streams, counted into the census of the branch
/// `schema` names under the types of the identity rows it locks:
///
/// ```sql
/// WITH changed_streams AS (INSERT INTO temporal_attribute_streams … RETURNING entity_key, attribute_name_key),
///      owners AS (SELECT … FROM entities … FOR SHARE)
/// INSERT INTO stream_type_census … FROM changed_streams JOIN owners …
/// ```
///
/// # Errors
///
/// As [`stream_census_change`].
pub(crate) fn counted_stream_addition(schema: Option<&DynIden>, mut addition: InsertStatement) -> Result<WithQuery, SeaQueryError> {
    addition.returning(Query::returning().columns(CHANGED_COLUMNS));
    let census = stream_census_change(schema, typed_by_owners(), StreamChange::Added)?;
    Ok(WithClause::new()
        .cte(CommonTableExpression::new().table_name(ChangedStreams).query(addition).to_owned())
        .cte(owners(schema))
        .to_owned()
        .query(census))
}

/// `removal`, a delete of streams, counted out of the census of the branch
/// `schema` names under the types of the identity rows it locks, answering
/// how many streams it removed:
///
/// ```sql
/// WITH changed_streams AS (DELETE FROM temporal_attribute_streams … RETURNING entity_key, attribute_name_key),
///      owners AS (SELECT … FROM entities … FOR SHARE),
///      census_change AS (INSERT INTO stream_type_census … FROM changed_streams JOIN owners …)
/// SELECT COUNT(*) FROM changed_streams
/// ```
///
/// The Evolution must still hold its types: a statement giving up the
/// Evolution runs after this one.
///
/// # Errors
///
/// As [`stream_census_change`].
pub(crate) fn counted_stream_removal(schema: Option<&DynIden>, mut removal: DeleteStatement) -> Result<WithQuery, SeaQueryError> {
    removal.returning(Query::returning().columns(CHANGED_COLUMNS));
    let census = stream_census_change(schema, typed_by_owners(), StreamChange::Removed)?;
    Ok(WithClause::new()
        .cte(CommonTableExpression::new().table_name(ChangedStreams).query(removal).to_owned())
        .cte(owners(schema))
        .cte(CommonTableExpression::new().table_name(CensusChange).query(census).to_owned())
        .to_owned()
        .query(Query::select().expr(count_all()).from(ChangedStreams).take()))
}

/// The streams of every Evolution in `gained` counted into the census of the
/// branch `schema` names under the type it gained:
///
/// ```sql
/// INSERT INTO stream_type_census …
/// FROM UNNEST($1::bigint[], $2::integer[]) AS gained(entity_key, entity_type_name_key)
/// JOIN temporal_attribute_streams AS gs ON gs.entity_key = gained.entity_key
/// ```
///
/// Run after the statement that gave the types, in a snapshot of its own,
/// while the transaction holds the identity rows: every stream committed
/// before the row was locked is counted here, and every stream written after
/// reads the gained type itself.
///
/// # Errors
///
/// As [`stream_census_change`].
pub(crate) fn gained_types_census(schema: Option<&DynIden>, gained: &[GainedType]) -> Result<UnnestAliased<InsertStatement>, SeaQueryError> {
    let columns = [
        UnnestColumn::new(Entities::EntityKey, PostgresType::BigintArray),
        UnnestColumn::new(StreamTypeCensus::EntityTypeNameKey, PostgresType::IntegerArray),
    ];
    let entity_keys: Vec<i64> = gained.iter().map(|pair| pair.entity_key).collect();
    let type_keys: Vec<i32> = gained.iter().map(|pair| pair.type_key).collect();
    let mut rows = Query::select();
    rows.expr_as(Expr::col((Gained, StreamTypeCensus::EntityTypeNameKey)), StreamTypeCensus::EntityTypeNameKey)
        .expr_as(
            Expr::col((GainingStream, TemporalAttributeStreams::AttributeNameKey)),
            StreamTypeCensus::AttributeNameKey,
        )
        .from_function(
            unnest_call([(&columns[0], Expr::val(entity_keys)), (&columns[1], Expr::val(type_keys))]),
            Gained,
        )
        .join_as(
            JoinType::Join,
            qualified(schema, TemporalAttributeStreams::Table),
            GainingStream,
            Expr::col((GainingStream, TemporalAttributeStreams::EntityKey)).equals((Gained, Entities::EntityKey)),
        );
    let census = stream_census_change(schema, rows.take(), StreamChange::Added)?;
    Ok(UnnestAliased::new(census).with_columns(Gained, &columns))
}

/// The columns a changed stream is counted by.
const CHANGED_COLUMNS: [TemporalAttributeStreams; 2] = [TemporalAttributeStreams::EntityKey, TemporalAttributeStreams::AttributeNameKey];

/// `owners`: the identity rows of the Entities of `changed_streams` in the
/// branch `schema` names, with their types, locked `FOR SHARE` in key order.
///
/// A lock waiting on a concurrent evolution upsert reads the row as that
/// upsert committed it, so the types it gained are read here.
fn owners(schema: Option<&DynIden>) -> CommonTableExpression {
    let mut changed = Query::select();
    changed.column(TemporalAttributeStreams::EntityKey).from(ChangedStreams);
    let mut rows = Query::select();
    rows.columns([Entities::EntityKey, Entities::EvolutionTypeNameKeys])
        .from(qualified(schema, Entities::Table))
        .and_where(Expr::col(Entities::EntityKey).in_subquery(changed.take()))
        .order_by(Entities::EntityKey, Order::Asc)
        .lock(LockType::Share);
    CommonTableExpression::new().table_name(Owners).query(rows.take()).to_owned()
}

/// The census rows of `changed_streams`: one per stream and type of its
/// Evolution, read from `owners`.
fn typed_by_owners() -> SelectStatement {
    let mut rows = Query::select();
    rows.from(ChangedStreams).join(
        JoinType::Join,
        Owners,
        Expr::col((Owners, Entities::EntityKey)).equals((ChangedStreams, TemporalAttributeStreams::EntityKey)),
    );
    typed_rows(
        rows,
        Expr::col((Owners, Entities::EvolutionTypeNameKeys)),
        Expr::col((ChangedStreams, TemporalAttributeStreams::AttributeNameKey)),
    )
}

/// `rows` projected as census rows: one per type of `types`, beside the
/// attribute key `attribute`.
#[must_use]
pub(crate) fn typed_rows(mut rows: SelectStatement, types: SimpleExpr, attribute: SimpleExpr) -> SelectStatement {
    rows.join_lateral(
        JoinType::InnerJoin,
        Query::select()
            .expr_as(SimpleExpr::from(unnest_of(types)), StreamTypeCensus::EntityTypeNameKey)
            .take(),
        Typed,
        true_literal(),
    )
    .expr_as(Expr::col((Typed, StreamTypeCensus::EntityTypeNameKey)), StreamTypeCensus::EntityTypeNameKey)
    .expr_as(attribute, StreamTypeCensus::AttributeNameKey);
    rows
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::stream_census::{GainedType, counted_stream_addition, counted_stream_removal, gained_types_census};
    use andromeda_postgresql_seaquery::statement::Statement;
    use pretty_assertions::assert_eq;
    use sea_query::{Alias, IntoIden, PostgresQueryBuilder, Query};
    use sqlx::Execute;

    fn removal_in(schema: Option<&sea_query::DynIden>) -> String {
        counted_stream_removal(schema, Query::delete().from_table(Alias::new("temporal_attribute_streams")).take())
            .expect("the removal builds")
            .to_string(PostgresQueryBuilder)
    }

    /// The statement text the driver receives for the census of `gained`.
    fn gained_census_in(schema: Option<&sea_query::DynIden>, gained: &[GainedType]) -> String {
        gained_types_census(schema, gained)
            .expect("the census builds")
            .to_query()
            .sql()
            .as_str()
            .to_owned()
    }

    /// A stream counts once under every type its Evolution is recorded under,
    /// read from the identity row the statement locks.
    #[test]
    fn a_removal_counts_the_streams_it_deleted_out_of_the_census_under_the_locked_types() {
        assert_eq!(
            removal_in(None),
            concat!(
                r#"WITH "changed_streams" AS (DELETE FROM "temporal_attribute_streams" RETURNING "entity_key", "attribute_name_key") , "#,
                r#""owners" AS (SELECT "entity_key", "evolution_type_name_keys" FROM "entities" "#,
                r#"WHERE "entity_key" IN (SELECT "entity_key" FROM "changed_streams") ORDER BY "entity_key" ASC FOR SHARE) , "#,
                r#""census_change" AS (INSERT INTO "stream_type_census" ("entity_type_name_key", "attribute_name_key", "stream_delta") "#,
                r#"SELECT "counted"."entity_type_name_key", "counted"."attribute_name_key", (COUNT(*)) * -1 FROM "#,
                r#"(SELECT "typed"."entity_type_name_key" AS "entity_type_name_key", "changed_streams"."attribute_name_key" AS "attribute_name_key" "#,
                r#"FROM "changed_streams" JOIN "owners" ON "owners"."entity_key" = "changed_streams"."entity_key" "#,
                r#"INNER JOIN LATERAL (SELECT UNNEST("owners"."evolution_type_name_keys") AS "entity_type_name_key") AS "typed" ON TRUE) AS "counted" "#,
                r#"GROUP BY "counted"."entity_type_name_key", "counted"."attribute_name_key") "#,
                r#"SELECT COUNT(*) FROM "changed_streams""#
            )
        );
    }

    #[test]
    fn an_addition_counts_the_streams_it_inserted_into_the_census_under_the_locked_types() {
        let mut addition = Query::insert();
        addition
            .into_table(Alias::new("temporal_attribute_streams"))
            .columns([Alias::new("entity_key")])
            .values_panic([1.into()]);
        let sql = counted_stream_addition(None, addition)
            .expect("the addition builds")
            .to_string(PostgresQueryBuilder);
        assert!(
            sql.starts_with(r#"WITH "changed_streams" AS (INSERT INTO "temporal_attribute_streams" ("entity_key") VALUES (1) RETURNING "entity_key", "attribute_name_key") , "owners" AS ("#),
            "{sql}"
        );
        assert!(sql.contains(r#"FOR SHARE) INSERT INTO "stream_type_census""#), "{sql}");
        assert!(
            sql.contains(r#"SELECT "counted"."entity_type_name_key", "counted"."attribute_name_key", COUNT(*) FROM"#),
            "{sql}"
        );
    }

    #[test]
    fn a_snapshot_branch_counts_into_its_own_census_and_locks_its_own_identity_rows() {
        let sql = removal_in(Some(&Alias::new("snap_4").into_iden()));
        assert!(sql.contains(r#"INSERT INTO "snap_4"."stream_type_census""#), "{sql}");
        assert!(sql.contains(r#"FROM "snap_4"."entities" WHERE"#), "{sql}");
    }

    /// A type an existing Evolution gains counts every stream the Evolution
    /// holds when the census runs, read from the stream relation itself.
    #[test]
    fn a_gained_type_counts_every_stream_of_its_evolution() {
        let gained = [GainedType { entity_key: 7, type_key: 3 }, GainedType { entity_key: 9, type_key: 4 }];
        assert_eq!(
            gained_census_in(None, &gained),
            concat!(
                r#"INSERT INTO "stream_type_census" ("entity_type_name_key", "attribute_name_key", "stream_delta") "#,
                r#"SELECT "counted"."entity_type_name_key", "counted"."attribute_name_key", COUNT(*) FROM "#,
                r#"(SELECT "gained"."entity_type_name_key" AS "entity_type_name_key", "gs"."attribute_name_key" AS "attribute_name_key" "#,
                r#"FROM UNNEST($1::bigint[], $2::integer[]) AS gained(entity_key, entity_type_name_key) "#,
                r#"JOIN "temporal_attribute_streams" AS "gs" ON "gs"."entity_key" = "gained"."entity_key") AS "counted" "#,
                r#"GROUP BY "counted"."entity_type_name_key", "counted"."attribute_name_key""#
            )
        );
    }

    #[test]
    fn a_snapshot_branch_counts_a_gained_type_over_its_own_streams() {
        let sql = gained_census_in(Some(&Alias::new("snap_4").into_iden()), &[GainedType { entity_key: 7, type_key: 3 }]);
        assert!(sql.starts_with(r#"INSERT INTO "snap_4"."stream_type_census""#), "{sql}");
        assert!(sql.contains(r#"JOIN "snap_4"."temporal_attribute_streams" AS "gs""#), "{sql}");
    }
}
