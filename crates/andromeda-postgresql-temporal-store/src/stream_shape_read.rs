//! Reading the families and value shapes each named attribute has been
//! recorded with, which clause 5.7.4.4 decides aggregation eligibility from.
//!
//! The recorded part is read from `temporal_stream_shapes`, so the read
//! touches no history row however long the history is. It skips through the
//! catalogue's attribute index from one combination of family and shape to
//! the next, so it costs one index step per combination an attribute holds,
//! however many streams hold it.
//!
//! On the live branch while its queue holds intents, the read answers as it
//! will once the drain applied them. The queued entries the application would
//! write add their own shapes. A queued entry carrying a client `instanceId`
//! replaces the recorded row of that instance on any dataset of its attribute
//! (clause 4.5.7), and only the rows tell whether another instance of the
//! stream still carries the replaced row's shape, so the streams of an Entity
//! attribute such an entry names are read from their rows, less the rows the
//! queue replaces. Those streams are few, and their history is all the read
//! touches.

use crate::{error::TemporalStoreError, repo::TemporalRepository, stored_attribute_shape::StoredAttributeShape, stored_catalog_error::StoredCatalogError};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_postgresql_codes::{attribute_kind::AttributeKind, recorded_shape::RecordedShape};
use andromeda_postgresql_query::{
    attribute_name_key::AttributeNameKey,
    history_source::{
        HistoryBranch,
        HistoryOf,
        attribute_rows::pending_attribute_rows,
        grain::HistoryGrain,
        instance_replacement::{kept_beside_pending, queued_instance_replacements},
    },
    tombstone::carries_a_value,
};
use andromeda_postgresql_schema::{
    name_dictionary::AttributeNames,
    temporal::{AttributeHistory, TemporalAttributeStreams, TemporalStreamShapes},
};
use andromeda_postgresql_seaquery::{branch::qualified, pg_array::is_any_of, statement::Statement};
use andromeda_postgresql_temporal_query::{
    alias::{Ah, An},
    recorded_shape::{RecordedShapeColumn, recorded_shape},
};
use sea_query::{
    CommonTableExpression,
    DynIden,
    Expr,
    ExprTrait,
    Iden,
    IntoIden,
    JoinType,
    Order,
    Query,
    SelectStatement,
    SimpleExpr,
    TableRef,
    UnionType,
    WithClause,
};
use sqlx::Row;
use std::collections::{HashMap, HashSet};

/// Alias `ss`: a row of `temporal_stream_shapes`.
#[derive(Iden, Clone, Copy)]
#[iden = "ss"]
struct Ss;

/// The combinations of family and shape the catalogue walk has found so far,
/// `held`: the recursive common table of [`catalogued_shapes`].
#[derive(Iden, Clone, Copy)]
#[iden = "held"]
struct Held;

/// Alias `step`: the next combination of one attribute the walk finds.
#[derive(Iden, Clone, Copy)]
#[iden = "step"]
struct Step;

/// Alias `rq`: an Entity attribute a queued entry replaces an instance of.
#[derive(Iden, Clone, Copy)]
#[iden = "rq"]
struct Replacement;

/// Alias `rs`: a recorded stream of such an Entity attribute.
#[derive(Iden, Clone, Copy)]
#[iden = "rs"]
struct ReplacedStream;

/// Alias `rp`: the streams of the Entity attributes queued entries replace an
/// instance of.
#[derive(Iden, Clone, Copy)]
#[iden = "rp"]
struct Replaced;

impl TemporalRepository {
    /// The family-and-value-shape combinations each named attribute has been
    /// recorded with across the whole history, tombstones excepted (they carry
    /// no value).
    ///
    /// The family travels with the shape because Table 4.5.19.1-3 is keyed on
    /// it alone: a Relationship and a `ListRelationship` have `distinctCount`
    /// and `totalCount` and nothing else, whichever column their targets were
    /// parked in. The shape itself is refined past the stored `value_kind`
    /// code, because Tables 4.5.19.1-1 and 4.5.19.1-2 give a JSON Array methods
    /// a JSON Object does not have and a `Time` an `avg` a `Date` does not
    /// have, while one JSONB column holds both of the first pair and one text
    /// column both of the second.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalStoreError::NameLookup`] when the dictionary read
    /// fails, [`TemporalStoreError::Database`] when the statement fails and
    /// [`TemporalStoreError::StoredCatalog`] when a stored name, family code or
    /// shape code is not one the dictionary or the registries know.
    pub(crate) async fn stored_attribute_shapes(
        &self,
        branch: &HistoryBranch,
        attributes: &[AttributeName],
    ) -> Result<HashMap<AttributeName, HashSet<StoredAttributeShape>>, TemporalStoreError> {
        if attributes.is_empty() {
            return Ok(HashMap::new());
        }
        // The names resolve through the shared dictionary cache, so the
        // statement narrows by key; a name never recorded has no shape.
        let keys: Vec<i32> = self
            .attribute_names
            .lookup(attributes)
            .await?
            .keys_of(attributes.iter().map(AttributeName::as_str))
            .into_iter()
            .map(AttributeNameKey::get)
            .collect();
        if keys.is_empty() {
            return Ok(HashMap::new());
        }
        let rows = recorded_attribute_shapes_query(branch, &keys).to_query().fetch_all(&self.pool).await?;
        let mut shapes_by_attribute: HashMap<AttributeName, HashSet<StoredAttributeShape>> = HashMap::new();
        for row in rows {
            let raw_name: String = row.try_get(Iden::to_string(&AttributeNames::Name).as_str())?;
            let name = raw_name.parse::<AttributeName>().map_err(StoredCatalogError::AttributeName)?;
            let stored_kind: i16 = row.try_get(Iden::to_string(&AttributeHistory::AttributeKind).as_str())?;
            let kind = AttributeKind::try_from(stored_kind).map_err(StoredCatalogError::AttributeKind)?;
            let code: i16 = row.try_get(Iden::to_string(&RecordedShapeColumn).as_str())?;
            let shape = RecordedShape::try_from(code).map_err(StoredCatalogError::RecordedShape)?;
            shapes_by_attribute.entry(name).or_default().insert(StoredAttributeShape { kind, shape });
        }
        Ok(shapes_by_attribute)
    }
}

/// The distinct `(name, family, shape)` triples the named attributes have been
/// recorded with in `branch`: those the stream catalogue holds, and where the
/// branch reads its queue, those of the streams queued entries replace an
/// instance of, read from their rows, and those of the queued entries.
fn recorded_attribute_shapes_query(branch: &HistoryBranch, keys: &[i32]) -> SelectStatement {
    let mut statement = catalogued_shapes(branch, keys);
    if let Some(replaced) = replaced_stream_shapes(branch, keys) {
        statement.union(UnionType::Distinct, replaced);
    }
    if let Some(queued) = queued_shapes(branch, keys) {
        statement.union(UnionType::Distinct, queued);
    }
    statement
}

/// The attribute under `an` is one of those keyed `keys`.
fn named(keys: &[i32]) -> SimpleExpr {
    is_any_of(Expr::col((An, AttributeNames::AttributeNameKey)), keys.iter().copied())
}

/// The combinations the stream catalogue of `branch` holds for the named
/// attributes, leaving out the streams queued entries replace an instance of.
///
/// A loose index scan: for each attribute the walk takes the lowest
/// combination its index range holds, then repeatedly the lowest above the one
/// it found, one index step each, until none is left. Probing each family and
/// shape pair instead leaves the plan to an estimate the planner cannot make
/// for a bound pair, and with one shape per attribute it takes every stream as
/// a match and scans; ordering each step by the index key keeps it on the
/// index.
fn catalogued_shapes(branch: &HistoryBranch, keys: &[i32]) -> SelectStatement {
    let columns = [
        TemporalStreamShapes::AttributeNameKey,
        TemporalStreamShapes::AttributeKind,
        TemporalStreamShapes::RecordedShape,
    ];
    let mut seed = Query::select();
    seed.columns(columns.map(|column| (Step, column)))
        .from_as(AttributeNames::Table, An)
        .join_lateral(
            JoinType::Join,
            next_combination(branch, Expr::col((An, AttributeNames::AttributeNameKey)), None),
            Step,
            SimpleExpr::from(true),
        )
        .and_where(named(keys));
    let mut walk = Query::select();
    walk.columns(columns.map(|column| (Step, column))).from(Held).join_lateral(
        JoinType::Join,
        next_combination(branch, Expr::col((Held, TemporalStreamShapes::AttributeNameKey)), Some(Held.into_iden())),
        Step,
        SimpleExpr::from(true),
    );
    seed.union(UnionType::All, walk);
    let held = CommonTableExpression::new()
        .table_name(Held)
        .columns(columns.map(IntoIden::into_iden))
        .query(seed)
        .to_owned();
    Query::select()
        .column((An, AttributeNames::Name))
        .expr_as(Expr::col((Held, TemporalStreamShapes::AttributeKind)), AttributeHistory::AttributeKind)
        .expr_as(Expr::col((Held, TemporalStreamShapes::RecordedShape)), RecordedShapeColumn)
        .from(Held)
        .join_as(
            JoinType::Join,
            AttributeNames::Table,
            An,
            Expr::col((An, AttributeNames::AttributeNameKey)).equals((Held, TemporalStreamShapes::AttributeNameKey)),
        )
        .with_cte(WithClause::new().recursive(true).cte(held).to_owned())
        .to_owned()
}

/// The lowest combination of family and shape the attribute `attribute_key`
/// holds in the stream catalogue of `branch`, above the one the row `after`
/// names when it names one, leaving out the streams queued entries replace an
/// instance of.
fn next_combination(branch: &HistoryBranch, attribute_key: SimpleExpr, after: Option<DynIden>) -> SelectStatement {
    let mut step = Query::select();
    step.column((Ss, TemporalStreamShapes::AttributeNameKey))
        .column((Ss, TemporalStreamShapes::AttributeKind))
        .column((Ss, TemporalStreamShapes::RecordedShape))
        .from(qualified(branch.schema(), TemporalStreamShapes::Table).alias(Ss))
        .and_where(Expr::col((Ss, TemporalStreamShapes::AttributeNameKey)).eq(attribute_key));
    if let Some(after) = after {
        step.and_where(
            Expr::tuple([
                Expr::col((Ss, TemporalStreamShapes::AttributeKind)),
                Expr::col((Ss, TemporalStreamShapes::RecordedShape)),
            ])
            .gt(Expr::tuple([
                // clone: the row names both columns of the combination it found.
                Expr::col((after.clone(), TemporalStreamShapes::AttributeKind)),
                Expr::col((after, TemporalStreamShapes::RecordedShape)),
            ])),
        );
    }
    if let Some(replaced) = replaced_streams(branch) {
        let mut keys = Query::select();
        keys.column((Replaced, TemporalAttributeStreams::StreamKey)).from_subquery(replaced, Replaced);
        step.and_where(Expr::col((Ss, TemporalStreamShapes::StreamKey)).not_in_subquery(keys));
    }
    step.order_by((Ss, TemporalStreamShapes::AttributeKind), Order::Asc)
        .order_by((Ss, TemporalStreamShapes::RecordedShape), Order::Asc)
        .limit(1)
        .to_owned()
}

/// The recorded streams of every Entity attribute a queued entry of `branch`
/// replaces an instance of, each with its key, its Evolution's identifier and
/// its attribute key; `None` where the branch reads no queue.
fn replaced_streams(branch: &HistoryBranch) -> Option<SelectStatement> {
    let replacements = queued_instance_replacements(branch)?;
    Some(
        Query::select()
            .column((ReplacedStream, TemporalAttributeStreams::StreamKey))
            .column((ReplacedStream, TemporalAttributeStreams::Id))
            .column((ReplacedStream, TemporalAttributeStreams::AttributeNameKey))
            .from_subquery(replacements, Replacement)
            .join_as(
                JoinType::Join,
                qualified(branch.schema(), TemporalAttributeStreams::Table),
                ReplacedStream,
                Expr::col((ReplacedStream, TemporalAttributeStreams::AttributeNameKey))
                    .equals((Replacement, HistoryGrain::AttributeNameKey))
                    .and(Expr::col((ReplacedStream, TemporalAttributeStreams::Id)).equals((Replacement, HistoryGrain::EntityId))),
            )
            .to_owned(),
    )
}

/// The combinations the rows of the streams queued entries replace an
/// instance of carry for the named attributes, less the rows the queue
/// replaces; `None` where `branch` reads no queue.
fn replaced_stream_shapes(branch: &HistoryBranch, keys: &[i32]) -> Option<SelectStatement> {
    let replaced = replaced_streams(branch)?;
    let mut select = Query::select();
    select
        .column((An, AttributeNames::Name))
        .column((Ah, AttributeHistory::AttributeKind))
        .expr_as(recorded_shape(), RecordedShapeColumn)
        .from_subquery(replaced, Replaced)
        .join_as(
            JoinType::Join,
            AttributeNames::Table,
            An,
            Expr::col((An, AttributeNames::AttributeNameKey)).equals((Replaced, TemporalAttributeStreams::AttributeNameKey)),
        )
        .join_as(
            JoinType::Join,
            qualified(branch.schema(), AttributeHistory::Table),
            Ah,
            Expr::col((Ah, AttributeHistory::StreamKey)).equals((Replaced, TemporalAttributeStreams::StreamKey)),
        )
        .and_where(named(keys))
        .and_where(carries_a_value(Ah));
    if let Some(kept) = kept_beside_pending(
        branch.pending(),
        Expr::col((Replaced, TemporalAttributeStreams::Id)),
        Expr::col((Replaced, TemporalAttributeStreams::AttributeNameKey)),
        Expr::col((Ah, AttributeHistory::VerbatimInstanceId)),
    ) {
        select.cond_where(kept);
    }
    Some(select)
}

/// The combinations the queued entries of `branch` the application would
/// write carry for the named attributes; `None` where the branch reads no
/// queue.
fn queued_shapes(branch: &HistoryBranch, keys: &[i32]) -> Option<SelectStatement> {
    let entries: TableRef = pending_attribute_rows(branch, &HistoryOf::EveryEntity, Ah)?;
    Some(
        Query::select()
            .column((An, AttributeNames::Name))
            .column((Ah, AttributeHistory::AttributeKind))
            .expr_as(recorded_shape(), RecordedShapeColumn)
            .from(entries)
            .join_as(
                JoinType::Join,
                AttributeNames::Table,
                An,
                Expr::col((An, AttributeNames::AttributeNameKey)).equals((Ah, HistoryGrain::AttributeNameKey)),
            )
            .and_where(named(keys))
            .and_where(carries_a_value(Ah))
            .to_owned(),
    )
}

#[cfg(test)]
mod tests {
    use crate::stream_shape_read::recorded_attribute_shapes_query;
    use andromeda_postgresql_query::history_source::HistoryBranch;
    use sea_query::{Alias, IntoIden, PostgresQueryBuilder};

    fn rendered(branch: &HistoryBranch) -> String {
        recorded_attribute_shapes_query(branch, &[7]).to_string(PostgresQueryBuilder)
    }

    /// Over no queue the shapes are the stream catalogue's alone, so the read
    /// touches no history row and no queued entry.
    #[test]
    fn over_no_queue_the_shapes_are_read_from_the_stream_catalogue_alone() {
        let sql = rendered(&HistoryBranch::live_recorded());
        assert!(sql.contains(r#"FROM "temporal_stream_shapes" AS "ss""#), "{sql}");
        for absent in ["attribute_history", "temporal_append"] {
            assert!(!sql.contains(absent), "{absent}: {sql}");
        }
        assert_eq!(sql.matches("UNION").count(), 1, "only the walk's own step is unioned: {sql}");
    }

    /// The walk steps through each attribute's index range from one held
    /// combination to the next, ordered by the index key so every step is one
    /// index descent, rather than reading every stream.
    #[test]
    fn the_catalogue_is_walked_one_held_combination_at_a_time() {
        let sql = rendered(&HistoryBranch::live_recorded());
        assert!(
            sql.starts_with(r#"WITH RECURSIVE "held" ("attribute_name_key", "attribute_kind", "recorded_shape") AS (SELECT "step"."attribute_name_key", "step"."attribute_kind", "step"."recorded_shape" FROM "attribute_names" AS "an" JOIN LATERAL (SELECT "ss"."attribute_name_key", "ss"."attribute_kind", "ss"."recorded_shape" FROM "temporal_stream_shapes" AS "ss" WHERE "ss"."attribute_name_key" = "an"."attribute_name_key" ORDER BY "ss"."attribute_kind" ASC, "ss"."recorded_shape" ASC LIMIT 1) AS "step" ON TRUE"#),
            "{sql}"
        );
        assert!(
            sql.contains(r#"FROM "held" JOIN LATERAL (SELECT "ss"."attribute_name_key", "ss"."attribute_kind", "ss"."recorded_shape" FROM "temporal_stream_shapes" AS "ss" WHERE "ss"."attribute_name_key" = "held"."attribute_name_key" AND ("ss"."attribute_kind", "ss"."recorded_shape") > ("held"."attribute_kind", "held"."recorded_shape") ORDER BY "ss"."attribute_kind" ASC, "ss"."recorded_shape" ASC LIMIT 1) AS "step" ON TRUE"#),
            "{sql}"
        );
    }

    #[test]
    fn a_snapshot_branch_reads_its_own_stream_catalogue() {
        let sql = rendered(&HistoryBranch::recorded_in(Alias::new("snap_1").into_iden()));
        assert!(sql.contains(r#"FROM "snap_1"."temporal_stream_shapes" AS "ss""#), "{sql}");
    }

    /// Clause 5.7.4.4 judges the instances that exist once the queue is
    /// applied: queued entries add their shapes, and a stream a queued client
    /// `instanceId` replaces a row of is read from its rows, less that row.
    #[test]
    fn over_a_queue_the_read_answers_as_the_applied_queue_will() {
        let sql = rendered(&HistoryBranch::live());
        assert_eq!(
            sql.matches(r#""ss"."stream_key" NOT IN (SELECT "rp"."stream_key" FROM (SELECT "rs"."stream_key""#)
                .count(),
            2,
            "{sql}"
        );
        assert!(
            sql.contains(r#"JOIN "attribute_history" AS "ah" ON "ah"."stream_key" = "rp"."stream_key""#),
            "{sql}"
        );
        assert!(
            sql.contains(r#""ah"."verbatim_instance_id" IS NULL OR (NOT EXISTS(SELECT 1 FROM "temporal_append_entries" AS "pr""#),
            "{sql}"
        );
        assert!(sql.contains(r#"FROM "temporal_append_entries" AS "pe""#), "{sql}");
        assert_eq!(sql.matches("UNION (").count(), 2, "{sql}");
    }

    /// Tombstones carry no value and so no shape.
    #[test]
    fn no_part_reads_a_shape_off_a_tombstone() {
        let sql = rendered(&HistoryBranch::live());
        assert_eq!(sql.matches(r#""ah"."value_kind" <> (0)"#).count(), 2, "{sql}");
    }
}
